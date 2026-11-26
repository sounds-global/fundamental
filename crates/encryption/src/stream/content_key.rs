use crate::{KeyPurpose, Keyring, SealedStreamError, StreamRefusal};
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::KeyInit;
use ring::rand::{SecureRandom, SystemRandom};
use std::fmt;
use zeroize::Zeroizing;

const KEY_LEN: usize = 32;

/// A random 256-bit key that seals one piece of content, and nothing else.
///
/// Drawn fresh per content and stored only [wrapped](Self::wrap) by a [`Keyring`], so deleting
/// the wrapped key makes the content unreadable.
pub struct ContentKey(Zeroizing<[u8; KEY_LEN]>);

impl ContentKey {
    /// Draws a fresh key.
    ///
    /// # Errors
    /// [`SealedStreamError::Entropy`] if the operating system cannot supply one.
    pub fn generate() -> Result<Self, SealedStreamError> {
        let mut key = Zeroizing::new([0_u8; KEY_LEN]);
        SystemRandom::new()
            .fill(key.as_mut_slice())
            .map_err(|_| SealedStreamError::Entropy)?;
        Ok(Self(key))
    }

    /// Seals this key under `keyring`, bound to `context`, so it unwraps on no other record.
    ///
    /// # Errors
    /// [`SealedStreamError::Keyring`] if the keyring cannot seal — including a purpose whose
    /// `MAX_PLAINTEXT_LEN` is below 32.
    pub fn wrap<P: KeyPurpose>(
        &self,
        keyring: &Keyring<P>,
        context: &[&[u8]],
    ) -> Result<Vec<u8>, SealedStreamError> {
        Ok(keyring.seal(self.0.as_slice(), context)?)
    }

    /// Opens a key [`wrap`](Self::wrap) sealed under the same keyring and context.
    ///
    /// # Errors
    /// [`SealedStreamError::Keyring`] if the keyring refuses it, or
    /// [`SealedStreamError::Refused`] if what opened is not a key.
    pub fn unwrap<P: KeyPurpose>(
        keyring: &Keyring<P>,
        wrapped: &[u8],
        context: &[&[u8]],
    ) -> Result<Self, SealedStreamError> {
        let opened = keyring.open(wrapped, context)?;
        let key: [u8; KEY_LEN] =
            opened
                .as_slice()
                .try_into()
                .map_err(|_| SealedStreamError::Refused {
                    reason: StreamRefusal::Length,
                })?;
        Ok(Self(Zeroizing::new(key)))
    }

    pub(crate) fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(self.0.as_slice().into())
    }
}

impl fmt::Debug for ContentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ContentKey([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::ContentKey;
    use crate::{KeyPurpose, Keyring, KeyringKeySettings, KeyringSettings, SealedStreamError};
    use fundamental_core::secrets::InMemorySecretString;

    struct Wrapping;
    impl KeyPurpose for Wrapping {
        const NAME: &'static str = "test.content-keys.v1";
        const MAX_PLAINTEXT_LEN: usize = 32;
    }

    fn keyring() -> Keyring<Wrapping> {
        let material = format!("{}:1:{}", Wrapping::NAME, "ab".repeat(32));
        Keyring::from_settings(&KeyringSettings {
            current: 1,
            versions: vec![KeyringKeySettings {
                version: 1,
                material: InMemorySecretString::new(material),
            }],
        })
        .expect("a keyring")
    }

    #[test]
    fn a_wrapped_key_unwraps_under_its_context() {
        let keyring = keyring();
        let key = ContentKey::generate().expect("entropy");
        let wrapped = key.wrap(&keyring, &[b"upload-1"]).expect("wrapped");
        let opened = ContentKey::unwrap(&keyring, &wrapped, &[b"upload-1"]).expect("unwrapped");
        assert_eq!(*opened.0, *key.0);
    }

    #[test]
    fn a_wrapped_key_opens_only_for_its_object() {
        let keyring = keyring();
        let key = ContentKey::generate().expect("entropy");
        let wrapped = key.wrap(&keyring, &[b"upload-1"]).expect("wrapped");
        assert!(matches!(
            ContentKey::unwrap(&keyring, &wrapped, &[b"upload-2"]),
            Err(SealedStreamError::Keyring(_))
        ));
    }

    #[test]
    fn two_keys_are_never_the_same() {
        let first = ContentKey::generate().expect("entropy");
        let second = ContentKey::generate().expect("entropy");
        assert_ne!(*first.0, *second.0);
    }

    #[test]
    fn a_key_never_prints() {
        let key = ContentKey::generate().expect("entropy");
        assert_eq!(format!("{key:?}"), "ContentKey([REDACTED])");
    }
}
