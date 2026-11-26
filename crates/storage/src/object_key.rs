//! The name an object is stored under.

use crate::{Private, Public, StoreError, Visibility};
use ring::rand::{SecureRandom, SystemRandom};
use std::fmt;
use std::marker::PhantomData;

const MAX_LEN: usize = 512;
const PRIVATE_PREFIX: &str = "p/";

/// The key of one object in a bucket of kind `V`: random for a private key, never a content
/// digest; caller-chosen for a public one. A key names one object for its life.
pub struct ObjectKey<V: Visibility> {
    key: String,
    kind: PhantomData<fn() -> V>,
}

impl ObjectKey<Private> {
    /// A fresh random key, `p/<2 hex>/<32 hex>`.
    ///
    /// # Errors
    /// [`StoreError::Sealing`] if the operating system cannot supply randomness.
    pub fn random() -> Result<Self, StoreError> {
        let mut bytes = [0_u8; 16];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| StoreError::Sealing {
                cause: fundamental_encryption::SealedStreamError::Entropy.into(),
            })?;
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(Self::trusted(format!(
            "{PRIVATE_PREFIX}{}/{hex}",
            &hex[..2]
        )))
    }

    /// Reads back a key [`random`](Self::random) made, as a record stored it.
    ///
    /// # Errors
    /// [`StoreError::InvalidSettings`] if `key` is not that shape.
    pub fn parse(key: &str) -> Result<Self, StoreError> {
        let shaped = key.strip_prefix(PRIVATE_PREFIX).and_then(|rest| {
            let (spread, hex) = rest.split_once('/')?;
            (spread.len() == 2
                && hex.len() == 32
                && hex.starts_with(spread)
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
            .then_some(())
        });
        shaped
            .map(|()| Self::trusted(key.to_owned()))
            .ok_or_else(|| invalid("not a private object key"))
    }
}

impl ObjectKey<Public> {
    /// A caller-chosen public key.
    ///
    /// # Errors
    /// [`StoreError::InvalidSettings`] if `key` is empty, longer than 512 bytes, under the
    /// private prefix, has an empty, `.` or `..` segment, or a byte outside the alphabet.
    pub fn parse(key: &str) -> Result<Self, StoreError> {
        if key.is_empty() || key.len() > MAX_LEN {
            return Err(invalid("a public key is 1 to 512 bytes"));
        }
        if key.starts_with(PRIVATE_PREFIX) {
            return Err(invalid("a public key cannot use the private prefix"));
        }
        for segment in key.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(invalid("a public key has an empty, `.` or `..` segment"));
            }
            if !segment
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
            {
                return Err(invalid("a public key uses a-z, 0-9, `.`, `_`, `-` and `/`"));
            }
        }
        Ok(Self::trusted(key.to_owned()))
    }
}

impl<V: Visibility> ObjectKey<V> {
    fn trusted(key: String) -> Self {
        Self {
            key,
            kind: PhantomData,
        }
    }

    /// The key as the provider stores it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.key
    }
}

impl<V: Visibility> Clone for ObjectKey<V> {
    fn clone(&self) -> Self {
        Self::trusted(self.key.clone())
    }
}

impl<V: Visibility> PartialEq for ObjectKey<V> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<V: Visibility> Eq for ObjectKey<V> {}

impl<V: Visibility> fmt::Debug for ObjectKey<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectKey<{}>({})", V::NAME, self.key)
    }
}

impl<V: Visibility> fmt::Display for ObjectKey<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key)
    }
}

fn invalid(problem: &str) -> StoreError {
    StoreError::InvalidSettings {
        problems: problem.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::ObjectKey;
    use crate::{Private, Public};

    #[test]
    fn a_random_key_parses_back_and_never_repeats() {
        let first = ObjectKey::<Private>::random().expect("entropy");
        let second = ObjectKey::<Private>::random().expect("entropy");
        assert_ne!(first, second);
        assert_eq!(
            ObjectKey::<Private>::parse(first.as_str()).expect("its shape"),
            first
        );
    }

    #[test]
    fn a_private_key_is_only_ever_random_shaped() {
        for wrong in [
            "",
            "p/",
            "p/ab/abcdef",
            "p/zz/zz000000000000000000000000000000",
            "p/ab/cd000000000000000000000000000000",
            "releases/app.dmg",
        ] {
            assert!(ObjectKey::<Private>::parse(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_public_key_keeps_to_its_alphabet() {
        assert!(ObjectKey::<Public>::parse("app/1.2.3/app_x64-setup.exe").is_ok());
        for wrong in ["", "p/ab/x", "a//b", "a/../b", "./a", "A/b", "a b", "a/b?c"] {
            assert!(ObjectKey::<Public>::parse(wrong).is_err(), "{wrong}");
        }
        assert!(ObjectKey::<Public>::parse(&"a".repeat(513)).is_err());
    }
}
