use crate::DigestContext;
use core::fmt;
use core::marker::PhantomData;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A secret key derived for one purpose, which opens that purpose's commitments and no other.
///
/// Each purpose gets its own key, so releasing one opens no other purpose's commitments.
pub struct OpeningKey<C: DigestContext> {
    bytes: [u8; 32],
    purpose: PhantomData<fn() -> C>,
}

impl<C: DigestContext> OpeningKey<C> {
    /// Derive `C`'s key from secret material, each part length-prefixed as a u64 in big-endian.
    #[must_use]
    pub fn derive(material: &[&[u8]]) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key(C::CONTEXT);
        for part in material {
            hasher.update(&(part.len() as u64).to_be_bytes());
            hasher.update(part);
        }
        Self {
            bytes: *hasher.finalize().as_bytes(),
            purpose: PhantomData,
        }
    }

    /// BLAKE3 `keyed_hash` of length-prefixed `parts` under this key: the commitment it opens.
    #[must_use]
    pub fn keyed_digest(&self, parts: &[&[u8]]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new_keyed(&self.bytes);
        for part in parts {
            hasher.update(&(part.len() as u64).to_be_bytes());
            hasher.update(part);
        }
        *hasher.finalize().as_bytes()
    }

    /// The key in its one released spelling: 43 characters of unpadded base64url.
    #[must_use]
    pub fn expose_base64url(&self) -> String {
        crate::base64url32::encode(&self.bytes)
    }

    /// Read a key a sealer released.
    ///
    /// # Errors
    ///
    /// [`CanonicalError::MalformedDigest`](crate::CanonicalError::MalformedDigest) for any
    /// spelling but the canonical one.
    pub fn from_base64url(text: &str) -> Result<Self, crate::CanonicalError> {
        crate::base64url32::decode(text)
            .map(|bytes| Self {
                bytes,
                purpose: PhantomData,
            })
            .ok_or(crate::CanonicalError::MalformedDigest)
    }
}

impl<C: DigestContext> Zeroize for OpeningKey<C> {
    fn zeroize(&mut self) {
        self.bytes.zeroize();
    }
}

impl<C: DigestContext> Drop for OpeningKey<C> {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl<C: DigestContext> ZeroizeOnDrop for OpeningKey<C> {}

impl<C: DigestContext> PartialEq for OpeningKey<C> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes.ct_eq(&other.bytes).into()
    }
}

impl<C: DigestContext> Eq for OpeningKey<C> {}

impl<C: DigestContext> fmt::Debug for OpeningKey<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "OpeningKey<{}>([REDACTED])", C::CONTEXT)
    }
}

#[cfg(test)]
mod tests {
    use super::OpeningKey;
    use crate::DigestContext;

    struct Leaf;
    impl DigestContext for Leaf {
        const CONTEXT: &'static str = "fundamental 2026-09-21 canonical test leaf";
    }

    struct Blinding;
    impl DigestContext for Blinding {
        const CONTEXT: &'static str = "fundamental 2026-09-21 canonical test blinding";
    }

    const SALT: &[u8] = &[7; 32];

    #[test]
    fn a_key_for_one_purpose_is_not_the_key_for_another() {
        let leaf = OpeningKey::<Leaf>::derive(&[SALT]);
        let blinding = OpeningKey::<Blinding>::derive(&[SALT]);
        assert_ne!(leaf.expose_base64url(), blinding.expose_base64url());
        assert_ne!(
            leaf.keyed_digest(&[b"account"]),
            blinding.keyed_digest(&[b"account"])
        );
    }

    #[test]
    fn the_derivation_is_frozen() {
        let key = OpeningKey::<Leaf>::derive(&[SALT]);
        let mut derive = blake3::Hasher::new_derive_key(Leaf::CONTEXT);
        derive.update(&32_u64.to_be_bytes());
        derive.update(SALT);
        let raw = *derive.finalize().as_bytes();
        let mut keyed = blake3::Hasher::new_keyed(&raw);
        keyed.update(&2_u64.to_be_bytes());
        keyed.update(b"ab");
        assert_eq!(key.keyed_digest(&[b"ab"]), *keyed.finalize().as_bytes());
    }

    #[test]
    fn length_prefixing_prevents_aliasing() {
        let key = OpeningKey::<Leaf>::derive(&[SALT]);
        assert_ne!(
            key.keyed_digest(&[b"ab", b"c"]),
            key.keyed_digest(&[b"a", b"bc"])
        );
    }

    #[test]
    fn a_released_key_reads_back_as_itself() {
        let key = OpeningKey::<Leaf>::derive(&[SALT]);
        let read = OpeningKey::<Leaf>::from_base64url(&key.expose_base64url());
        assert_eq!(read.ok(), Some(key));
    }

    #[test]
    fn debug_never_prints_the_key() {
        let rendered = format!("{:?}", OpeningKey::<Leaf>::derive(&[SALT]));
        assert_eq!(
            rendered,
            "OpeningKey<fundamental 2026-09-21 canonical test leaf>([REDACTED])"
        );
    }
}
