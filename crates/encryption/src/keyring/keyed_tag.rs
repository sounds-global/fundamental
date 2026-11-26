use crate::{KeyPurpose, KeyringVersion};
use std::fmt;
use std::marker::PhantomData;
use subtle::ConstantTimeEq;

/// A 32-byte keyed digest produced by [`Keyring::tag`](crate::Keyring::tag).
///
/// Typed by purpose; equality is constant-time; `Debug` never shows the bytes.
pub struct KeyedTag<P: KeyPurpose> {
    version: KeyringVersion,
    bytes: [u8; 32],
    purpose: PhantomData<fn() -> P>,
}

impl<P: KeyPurpose> KeyedTag<P> {
    /// A tag read back from storage, with the version stored beside it.
    #[must_use]
    pub fn from_parts(version: KeyringVersion, bytes: [u8; 32]) -> Self {
        Self {
            version,
            bytes,
            purpose: PhantomData,
        }
    }

    /// The version of the key that produced this tag.
    #[must_use]
    pub fn version(&self) -> KeyringVersion {
        self.version
    }

    /// The digest.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

impl<P: KeyPurpose> Clone for KeyedTag<P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P: KeyPurpose> Copy for KeyedTag<P> {}

impl<P: KeyPurpose> ConstantTimeEq for KeyedTag<P> {
    fn ct_eq(&self, other: &Self) -> subtle::Choice {
        self.version.get().ct_eq(&other.version.get()) & self.bytes.ct_eq(&other.bytes)
    }
}

impl<P: KeyPurpose> PartialEq for KeyedTag<P> {
    fn eq(&self, other: &Self) -> bool {
        bool::from(self.ct_eq(other))
    }
}

impl<P: KeyPurpose> Eq for KeyedTag<P> {}

impl<P: KeyPurpose> fmt::Debug for KeyedTag<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyedTag")
            .field("purpose", &P::NAME)
            .field("version", &self.version)
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::KeyedTag;
    use crate::{KeyPurpose, KeyringVersion};

    struct Digests;
    impl KeyPurpose for Digests {
        const NAME: &'static str = "test.digests";
        const MAX_PLAINTEXT_LEN: usize = 0;
    }

    fn version(number: u16) -> KeyringVersion {
        KeyringVersion::new(number).expect("a non-zero test version")
    }

    fn tag(number: u16, byte: u8) -> KeyedTag<Digests> {
        KeyedTag::from_parts(version(number), [byte; 32])
    }

    #[test]
    fn tags_are_equal_only_when_version_and_bytes_are() {
        assert_eq!(tag(1, 7), tag(1, 7));
        assert_ne!(tag(1, 7), tag(2, 7));
        assert_ne!(tag(1, 7), tag(1, 8));
    }

    #[test]
    fn debug_never_prints_the_digest() {
        let rendered = format!("{:?}", tag(3, 0xab));
        assert!(!rendered.contains("171"), "{rendered}");
        assert!(rendered.contains("REDACTED"), "{rendered}");
        assert!(rendered.contains("test.digests"), "{rendered}");
    }
}
