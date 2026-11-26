use crate::stream::segment_sealer::associated_data;
use crate::stream::stream_header::StreamHeader;
use crate::{ContentKey, SealedStreamError, StreamLayout, StreamRefusal};
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::stream::{NewStream, StreamBE32, StreamPrimitive};
use zeroize::Zeroizing;

/// Opens the segments of one sealed stream.
///
/// By position, so a byte range opens only its segments; a moved, dropped, repeated, cut or
/// appended segment fails to authenticate.
pub struct SegmentOpener {
    stream: StreamBE32<XChaCha20Poly1305>,
    associated: Vec<u8>,
}

impl SegmentOpener {
    /// Starts reading a stream sealed under `key` and `binding`, whose first
    /// [`HEADER_LEN`](StreamLayout::HEADER_LEN) bytes are `header`.
    ///
    /// # Errors
    /// [`SealedStreamError::Refused`] if the header is not this format.
    pub fn begin(
        key: &ContentKey,
        binding: &[&[u8]],
        header: &[u8],
    ) -> Result<Self, SealedStreamError> {
        let header = StreamHeader::parse(header)?;
        Ok(Self {
            stream: StreamBE32::from_aead(key.cipher(), header.nonce_prefix().into()),
            associated: associated_data(&header, binding),
        })
    }

    /// Opens segment `index` of a stream sealing `plaintext_len` bytes.
    ///
    /// # Errors
    /// [`SealedStreamError::Refused`] if the stream has no such segment, `sealed` is not its
    /// length, or it does not authenticate there.
    pub fn open(
        &self,
        index: u64,
        plaintext_len: u64,
        sealed: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, SealedStreamError> {
        let refused = |reason| SealedStreamError::Refused { reason };
        let (_, expected) = StreamLayout::segment_range(index, plaintext_len)
            .ok_or_else(|| refused(StreamRefusal::Length))?;
        if u64::try_from(sealed.len()).ok() != Some(expected) {
            return Err(refused(StreamRefusal::Length));
        }
        let position = u32::try_from(index).map_err(|_| refused(StreamRefusal::Length))?;
        let last = index + 1 == StreamLayout::segment_count(plaintext_len);
        let mut buffer = Zeroizing::new(sealed.to_vec());
        self.stream
            .decrypt_in_place(position, last, &self.associated, &mut *buffer)
            .map_err(|_| refused(StreamRefusal::Unauthenticated))?;
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::SegmentOpener;
    use crate::{ContentKey, SealedStreamError, SegmentSealer, StreamLayout, StreamRefusal};

    const SEGMENT: usize = StreamLayout::SEGMENT_LEN;

    fn sealed(key: &ContentKey, binding: &[&[u8]], plaintext: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
        let (mut sealer, header) = SegmentSealer::begin(key, binding).expect("begun");
        let mut segments: Vec<Vec<u8>> = sealer
            .update(plaintext)
            .expect("sealed")
            .into_iter()
            .map(|segment| segment.into_bytes())
            .collect();
        segments.push(sealer.finish().expect("last").into_bytes());
        (header.into_bytes(), segments)
    }

    fn plaintext(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    fn open_all(
        key: &ContentKey,
        binding: &[&[u8]],
        header: &[u8],
        segments: &[Vec<u8>],
        len: usize,
    ) -> Result<Vec<u8>, SealedStreamError> {
        let opener = SegmentOpener::begin(key, binding, header)?;
        let mut out = Vec::new();
        for (index, segment) in segments.iter().enumerate() {
            out.extend_from_slice(&opener.open(index as u64, len as u64, segment)?);
        }
        Ok(out)
    }

    fn refused(result: Result<Vec<u8>, SealedStreamError>, reason: StreamRefusal) {
        match result {
            Err(SealedStreamError::Refused { reason: got }) => assert_eq!(got, reason),
            other => panic!("expected a {reason} refusal, got {other:?}"),
        }
    }

    #[test]
    fn round_trips_at_every_boundary() {
        let key = ContentKey::generate().expect("entropy");
        for len in [0, 1, SEGMENT - 1, SEGMENT, SEGMENT + 1, 3 * 1024 * 1024] {
            let content = plaintext(len);
            let (header, segments) = sealed(&key, &[b"a"], &content);
            let opened = open_all(&key, &[b"a"], &header, &segments, len).expect("opens");
            assert_eq!(opened, content, "{len}");
        }
    }

    #[test]
    fn a_truncated_stream_is_refused() {
        let key = ContentKey::generate().expect("entropy");
        let len = 2 * SEGMENT + 3;
        let (header, segments) = sealed(&key, &[b"a"], &plaintext(len));
        // Dropping the last segment: the one now last was sealed as not-last.
        let shorter = 2 * SEGMENT;
        refused(
            open_all(&key, &[b"a"], &header, &segments[..2], shorter),
            StreamRefusal::Unauthenticated,
        );
        let mut cut = segments.clone();
        cut[2].truncate(5);
        refused(
            open_all(&key, &[b"a"], &header, &cut, len),
            StreamRefusal::Length,
        );
    }

    #[test]
    fn an_extended_stream_is_refused() {
        let key = ContentKey::generate().expect("entropy");
        let len = SEGMENT;
        let (header, mut segments) = sealed(&key, &[b"a"], &plaintext(len));
        let last = segments[0].clone();
        segments.push(last);
        refused(
            open_all(&key, &[b"a"], &header, &segments, 2 * SEGMENT),
            StreamRefusal::Unauthenticated,
        );
    }

    #[test]
    fn a_reordered_segment_is_refused() {
        let key = ContentKey::generate().expect("entropy");
        let len = 3 * SEGMENT + 1;
        let (header, mut segments) = sealed(&key, &[b"a"], &plaintext(len));
        segments.swap(0, 1);
        refused(
            open_all(&key, &[b"a"], &header, &segments, len),
            StreamRefusal::Unauthenticated,
        );
    }

    #[test]
    fn a_segment_of_another_object_is_refused() {
        let key = ContentKey::generate().expect("entropy");
        let len = SEGMENT + 9;
        let (header, segments) = sealed(&key, &[b"a"], &plaintext(len));
        let (_, other) = sealed(&key, &[b"a"], &plaintext(len));
        let mixed = vec![other[0].clone(), segments[1].clone()];
        refused(
            open_all(&key, &[b"a"], &header, &mixed, len),
            StreamRefusal::Unauthenticated,
        );
        refused(
            open_all(&key, &[b"b"], &header, &segments, len),
            StreamRefusal::Unauthenticated,
        );
        let stranger = ContentKey::generate().expect("entropy");
        refused(
            open_all(&stranger, &[b"a"], &header, &segments, len),
            StreamRefusal::Unauthenticated,
        );
    }

    #[test]
    fn a_flipped_bit_is_refused() {
        let key = ContentKey::generate().expect("entropy");
        let (header, mut segments) = sealed(&key, &[b"a"], &plaintext(10));
        segments[0][3] ^= 1;
        refused(
            open_all(&key, &[b"a"], &header, &segments, 10),
            StreamRefusal::Unauthenticated,
        );
    }

    #[test]
    fn one_segment_opens_without_the_ones_before_it() {
        let key = ContentKey::generate().expect("entropy");
        let len = 3 * SEGMENT + 100;
        let content = plaintext(len);
        let (header, segments) = sealed(&key, &[b"a"], &content);
        let opener = SegmentOpener::begin(&key, &[b"a"], &header).expect("header");
        let third = opener.open(2, len as u64, &segments[2]).expect("opens");
        assert_eq!(&third[..], &content[2 * SEGMENT..3 * SEGMENT]);
    }
}
