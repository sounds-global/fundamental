use crate::{SealedStreamError, StreamLayout, StreamRefusal};

/// `magic(3) ‖ format(1) ‖ log2(segment)(1) ‖ nonce prefix(19)`.
///
/// Part of every segment's associated data; the nonce prefix is fresh per stream.
pub(crate) struct StreamHeader {
    bytes: [u8; StreamLayout::HEADER_LEN],
}

const MAGIC: [u8; 3] = *b"FSS";
const FORMAT: u8 = 1;
/// XChaCha20-Poly1305's 24-byte nonce, less STREAM's 4-byte counter and 1-byte last flag.
pub(crate) const NONCE_PREFIX_LEN: usize = 19;

impl StreamHeader {
    pub(crate) fn new(nonce_prefix: [u8; NONCE_PREFIX_LEN]) -> Self {
        let mut bytes = [0_u8; StreamLayout::HEADER_LEN];
        bytes[..3].copy_from_slice(&MAGIC);
        bytes[3] = FORMAT;
        bytes[4] = StreamLayout::SEGMENT_LOG2;
        bytes[5..].copy_from_slice(&nonce_prefix);
        Self { bytes }
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, SealedStreamError> {
        let refused = SealedStreamError::Refused {
            reason: StreamRefusal::Header,
        };
        let bytes: [u8; StreamLayout::HEADER_LEN] = bytes.try_into().map_err(|_| refused)?;
        if bytes[..3] != MAGIC || bytes[3] != FORMAT || bytes[4] != StreamLayout::SEGMENT_LOG2 {
            return Err(SealedStreamError::Refused {
                reason: StreamRefusal::Header,
            });
        }
        Ok(Self { bytes })
    }

    pub(crate) fn as_bytes(&self) -> &[u8; StreamLayout::HEADER_LEN] {
        &self.bytes
    }

    pub(crate) fn nonce_prefix(&self) -> &[u8] {
        &self.bytes[5..]
    }
}

#[cfg(test)]
mod tests {
    use super::{NONCE_PREFIX_LEN, StreamHeader};
    use crate::{SealedStreamError, StreamRefusal};

    #[test]
    fn a_header_parses_back_to_its_prefix() {
        let header = StreamHeader::new([7; NONCE_PREFIX_LEN]);
        let parsed = StreamHeader::parse(header.as_bytes()).expect("its own header");
        assert_eq!(parsed.nonce_prefix(), &[7; NONCE_PREFIX_LEN]);
    }

    #[test]
    fn another_format_or_segment_size_is_refused() {
        let header = StreamHeader::new([0; NONCE_PREFIX_LEN]);
        for (index, value) in [(0, b'X'), (3, 2), (4, 12)] {
            let mut bytes = *header.as_bytes();
            bytes[index] = value;
            assert!(matches!(
                StreamHeader::parse(&bytes),
                Err(SealedStreamError::Refused {
                    reason: StreamRefusal::Header
                })
            ));
        }
        assert!(StreamHeader::parse(&header.as_bytes()[..23]).is_err());
    }
}
