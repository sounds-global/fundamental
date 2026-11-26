use crate::fns::length_prefixed;
use crate::stream::stream_header::{NONCE_PREFIX_LEN, StreamHeader};
use crate::{ContentKey, SealedSegment, SealedStreamError, StreamLayout};
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::stream::{NewStream, StreamBE32, StreamPrimitive};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;

/// Seals a stream of any length while holding at most one segment of it.
///
/// Only [`finish`](Self::finish) seals a segment as last, so a stream cut short never opens.
pub struct SegmentSealer {
    stream: StreamBE32<XChaCha20Poly1305>,
    associated: Vec<u8>,
    pending: Zeroizing<Vec<u8>>,
    position: u32,
}

impl SegmentSealer {
    /// Starts a stream under `key`, bound to `binding` (such as its record id and object key);
    /// answers the sealer and the header.
    ///
    /// # Errors
    /// [`SealedStreamError::Entropy`] if no nonce prefix can be drawn.
    pub fn begin(
        key: &ContentKey,
        binding: &[&[u8]],
    ) -> Result<(Self, SealedSegment), SealedStreamError> {
        let mut prefix = [0_u8; NONCE_PREFIX_LEN];
        SystemRandom::new()
            .fill(&mut prefix)
            .map_err(|_| SealedStreamError::Entropy)?;
        let header = StreamHeader::new(prefix);
        let sealer = Self {
            stream: StreamBE32::from_aead(key.cipher(), header.nonce_prefix().into()),
            associated: associated_data(&header, binding),
            pending: Zeroizing::new(Vec::with_capacity(StreamLayout::SEGMENT_LEN)),
            position: 0,
        };
        Ok((sealer, SealedSegment::new(header.as_bytes().to_vec())))
    }

    /// Takes the next `bytes` of content, and answers every segment now known not to be last.
    ///
    /// # Errors
    /// [`SealedStreamError::TooLong`] past the counter's last segment, or
    /// [`SealedStreamError::Encryption`] if the cipher refuses.
    pub fn update(&mut self, bytes: &[u8]) -> Result<Vec<SealedSegment>, SealedStreamError> {
        self.pending.extend_from_slice(bytes);
        let full = (self.pending.len().saturating_sub(1)) / StreamLayout::SEGMENT_LEN;
        let mut sealed = Vec::with_capacity(full);
        for index in 0..full {
            let start = index * StreamLayout::SEGMENT_LEN;
            let segment = &self.pending[start..start + StreamLayout::SEGMENT_LEN];
            sealed.push(seal(
                &self.stream,
                self.position,
                false,
                &self.associated,
                segment,
            )?);
            self.position = self
                .position
                .checked_add(1)
                .ok_or(SealedStreamError::TooLong)?;
        }
        self.pending.drain(..full * StreamLayout::SEGMENT_LEN);
        Ok(sealed)
    }

    /// Seals what remains as the last segment — empty when the content was.
    ///
    /// # Errors
    /// [`SealedStreamError::Encryption`] if the cipher refuses.
    pub fn finish(self) -> Result<SealedSegment, SealedStreamError> {
        seal(
            &self.stream,
            self.position,
            true,
            &self.associated,
            &self.pending,
        )
    }
}

fn seal(
    stream: &StreamBE32<XChaCha20Poly1305>,
    position: u32,
    last: bool,
    associated: &[u8],
    plaintext: &[u8],
) -> Result<SealedSegment, SealedStreamError> {
    let mut buffer = Vec::with_capacity(plaintext.len() + StreamLayout::TAG_LEN);
    buffer.extend_from_slice(plaintext);
    stream
        .encrypt_in_place(position, last, associated, &mut buffer)
        .map_err(|_| SealedStreamError::Encryption)?;
    Ok(SealedSegment::new(buffer))
}

/// The header, then the caller's binding, length-prefixed so no two bindings frame alike.
pub(crate) fn associated_data(header: &StreamHeader, binding: &[&[u8]]) -> Vec<u8> {
    let mut associated = header.as_bytes().to_vec();
    associated.extend_from_slice(&length_prefixed(binding.iter().copied()));
    associated
}

#[cfg(test)]
mod tests {
    use super::SegmentSealer;
    use crate::{ContentKey, StreamLayout};

    #[test]
    fn a_segment_is_held_until_more_bytes_follow_it() {
        let key = ContentKey::generate().expect("entropy");
        let (mut sealer, header) = SegmentSealer::begin(&key, &[b"one"]).expect("begun");
        assert_eq!(header.len(), StreamLayout::HEADER_LEN);
        let exactly_one = vec![1_u8; StreamLayout::SEGMENT_LEN];
        assert!(sealer.update(&exactly_one).expect("sealed").is_empty());
        let next = sealer.update(&[2]).expect("sealed");
        assert_eq!(next.len(), 1);
        assert_eq!(
            next[0].len(),
            StreamLayout::SEGMENT_LEN + StreamLayout::TAG_LEN
        );
        assert_eq!(
            sealer.finish().expect("last").len(),
            1 + StreamLayout::TAG_LEN
        );
    }

    #[test]
    fn the_sealed_length_is_the_layout_s() {
        let key = ContentKey::generate().expect("entropy");
        for len in [
            0,
            1,
            StreamLayout::SEGMENT_LEN,
            3 * StreamLayout::SEGMENT_LEN + 7,
        ] {
            let (mut sealer, header) = SegmentSealer::begin(&key, &[]).expect("begun");
            let mut total = header.len();
            for chunk in vec![9_u8; len].chunks(10_000) {
                total += sealer
                    .update(chunk)
                    .expect("sealed")
                    .iter()
                    .map(|s| s.len())
                    .sum::<usize>();
            }
            total += sealer.finish().expect("last").len();
            assert_eq!(
                Some(total as u64),
                StreamLayout::ciphertext_len(len as u64),
                "{len}"
            );
        }
    }
}
