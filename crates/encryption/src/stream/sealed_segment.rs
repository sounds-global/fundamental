/// A piece of a sealed stream — its header or one sealed segment — exactly as a
/// [`SegmentSealer`](crate::SegmentSealer) produced it.
///
/// Only this crate builds one, so a store taking `SealedSegment`s only ever holds ciphertext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedSegment(Vec<u8>);

impl SealedSegment {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// The sealed bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The sealed bytes, owned.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// How many bytes it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether it holds no bytes — never true of anything a sealer produces.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
