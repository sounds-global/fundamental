use crate::DigestContext;
use core::fmt;
use core::marker::PhantomData;
use subtle::ConstantTimeEq;

/// A BLAKE3 `derive_key` digest over length-prefixed parts, typed by its purpose.
///
/// Each part is length-prefixed, so `["ab", "c"]` and `["a", "bc"]` never derive the same digest.
pub struct ContextDigest<C: DigestContext> {
    bytes: [u8; 32],
    purpose: PhantomData<fn() -> C>,
}

impl<C: DigestContext> ContextDigest<C> {
    /// Derive the digest of `parts` under `C`'s context.
    #[must_use]
    pub fn derive(parts: &[&[u8]]) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key(C::CONTEXT);
        for part in parts {
            hasher.update(&(part.len() as u64).to_be_bytes());
            hasher.update(part);
        }
        Self {
            bytes: *hasher.finalize().as_bytes(),
            purpose: PhantomData,
        }
    }

    /// The bare form a digest travels in: 43 characters of unpadded base64url.
    #[must_use]
    pub fn to_base64url(&self) -> String {
        crate::base64url32::encode(&self.bytes)
    }

    /// Read a digest received from elsewhere, to compare with one derived here.
    ///
    /// # Errors
    ///
    /// [`CanonicalError::MalformedDigest`](crate::CanonicalError::MalformedDigest) for any
    /// spelling but the canonical one.
    pub fn parse_base64url(text: &str) -> Result<Self, crate::CanonicalError> {
        crate::base64url32::decode(text)
            .map(|bytes| Self {
                bytes,
                purpose: PhantomData,
            })
            .ok_or(crate::CanonicalError::MalformedDigest)
    }

    /// The full 32 bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    /// The first `N` bytes, for a key column narrower than the digest — a UUID takes 16.
    #[must_use]
    pub fn prefix<const N: usize>(&self) -> [u8; N] {
        const { assert!(N <= 32, "a prefix cannot be longer than the 32-byte digest") };
        let mut out = [0_u8; N];
        out.copy_from_slice(&self.bytes[..N]);
        out
    }
}

impl<C: DigestContext> Clone for ContextDigest<C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C: DigestContext> Copy for ContextDigest<C> {}

impl<C: DigestContext> PartialEq for ContextDigest<C> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes.ct_eq(&other.bytes).into()
    }
}

impl<C: DigestContext> Eq for ContextDigest<C> {}

impl<C: DigestContext> core::hash::Hash for ContextDigest<C> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

/// Redacted: a derived key can be a correlation handle, and a log is not where it belongs.
impl<C: DigestContext> fmt::Debug for ContextDigest<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ContextDigest<{}>(..)", C::CONTEXT)
    }
}

#[cfg(test)]
mod tests {
    use super::ContextDigest;
    use crate::DigestContext;

    struct First;
    impl DigestContext for First {
        const CONTEXT: &'static str = "fundamental 2026-09-19 canonical test first";
    }

    struct Second;
    impl DigestContext for Second {
        const CONTEXT: &'static str = "fundamental 2026-09-19 canonical test second";
    }

    #[test]
    fn length_prefixing_prevents_aliasing() {
        assert_ne!(
            ContextDigest::<First>::derive(&[b"ab", b"c"]),
            ContextDigest::<First>::derive(&[b"a", b"bc"])
        );
        assert_ne!(
            ContextDigest::<First>::derive(&[b"", b"a"]),
            ContextDigest::<First>::derive(&[b"a", b""])
        );
    }

    #[test]
    fn two_contexts_never_share_a_digest() {
        let first = ContextDigest::<First>::derive(&[b"x"]);
        let second = ContextDigest::<Second>::derive(&[b"x"]);
        assert_ne!(first.as_bytes(), second.as_bytes());
    }

    #[test]
    fn the_derivation_is_frozen() {
        let digest = ContextDigest::<First>::derive(&[b"x"]);
        let mut hasher = blake3::Hasher::new_derive_key(First::CONTEXT);
        hasher.update(&1_u64.to_be_bytes());
        hasher.update(b"x");
        assert_eq!(digest.as_bytes(), hasher.finalize().as_bytes());
        assert_eq!(digest.prefix::<16>(), digest.as_bytes()[..16]);
    }

    #[test]
    fn the_bare_spelling_round_trips() {
        let digest = ContextDigest::<First>::derive(&[b"x"]);
        assert_eq!(
            ContextDigest::<First>::parse_base64url(&digest.to_base64url()).ok(),
            Some(digest)
        );
    }

    #[test]
    fn debug_never_prints_the_bytes() {
        let rendered = format!("{:?}", ContextDigest::<First>::derive(&[b"x"]));
        assert_eq!(
            rendered,
            "ContextDigest<fundamental 2026-09-19 canonical test first>(..)"
        );
    }
}
