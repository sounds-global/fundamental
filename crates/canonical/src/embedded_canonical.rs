use crate::{CanonicalError, Sha256Digest};

/// Canonical bytes compiled into the binary, with the digest a build already checked.
///
/// Made only by `fundamental::embed_canonical!`, which checks the digest at compile time.
#[derive(Clone, Copy, Debug)]
pub struct EmbeddedCanonical {
    bytes: &'static [u8],
    digest: Sha256Digest,
}

impl EmbeddedCanonical {
    /// Only `embed_canonical!` calls this, after it has checked `bytes` against `digest`.
    #[doc(hidden)]
    #[must_use]
    pub const fn __checked_at_build(bytes: &'static [u8], digest: Sha256Digest) -> Self {
        Self { bytes, digest }
    }

    /// The embedded bytes.
    #[must_use]
    pub const fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// The digest the build checked them against.
    #[must_use]
    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    /// Re-hash the bytes the binary actually carries.
    ///
    /// # Errors
    ///
    /// [`CanonicalError::DigestMismatch`] when they no longer match.
    pub fn verify(&self) -> Result<(), CanonicalError> {
        self.digest.verify(self.bytes)
    }
}
