use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use keyring::Entry;
use zeroize::Zeroize;

use crate::KeyError;
use crate::hd::seal_aead::{seal_seed, unseal_seed};
use crate::hd::{MasterSeed, SealedSeed, SeedSealer};

const WRAP_ACCOUNT: &str = "hd-seed-wrap";

/// A [`SeedSealer`] that encrypts the master seed under a random AES-256 key held in
/// the OS keychain; the [`SealedSeed`] is a portable blob.
pub struct KeychainSealer {
    service: String,
}

impl KeychainSealer {
    /// Creates a sealer keeping its wrapping key under the keychain `service` name.
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self) -> Result<Entry, KeyError> {
        Entry::new(&self.service, WRAP_ACCOUNT)
            .map_err(|e| KeyError::Seal(format!("keychain entry: {e}")))
    }

    fn load_wrapping_key(&self) -> Result<Option<[u8; 32]>, KeyError> {
        match self.entry()?.get_password() {
            Ok(encoded) => {
                let bytes = STANDARD_NO_PAD
                    .decode(encoded)
                    .map_err(|_| KeyError::Unseal)?;
                let key: [u8; 32] = bytes.try_into().map_err(|_| KeyError::Unseal)?;
                Ok(Some(key))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeyError::Seal(format!("keychain read: {e}"))),
        }
    }

    fn load_or_create_wrapping_key(&self) -> Result<[u8; 32], KeyError> {
        if let Some(key) = self.load_wrapping_key()? {
            return Ok(key);
        }
        let mut key = [0u8; 32];
        getrandom::getrandom(&mut key)
            .map_err(|_| KeyError::Seal("wrapping key entropy failed".into()))?;
        let mut encoded = STANDARD_NO_PAD.encode(key);
        self.entry()?
            .set_password(&encoded)
            .map_err(|e| KeyError::Seal(format!("keychain write: {e}")))?;
        encoded.zeroize();
        Ok(key)
    }
}

impl SeedSealer for KeychainSealer {
    async fn seal(&self, seed: &MasterSeed) -> Result<SealedSeed, KeyError> {
        let mut key = self.load_or_create_wrapping_key()?;
        let sealed = seal_seed(&key, Vec::new(), seed);
        key.zeroize();
        sealed
    }

    async fn unseal(&self, sealed: &SealedSeed) -> Result<MasterSeed, KeyError> {
        // A salt marks a password envelope; refused explicitly rather than by the AEAD tag.
        if !sealed.salt().is_empty() {
            return Err(KeyError::Unseal);
        }
        let mut key = self.load_wrapping_key()?.ok_or(KeyError::Unseal)?;
        let recovered = unseal_seed(&key, sealed);
        key.zeroize();
        recovered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Uses the real OS keychain, so opt-in with `--ignored`.
    #[tokio::test]
    #[ignore = "requires an OS keychain"]
    async fn round_trips_through_the_real_keychain() {
        let sealer = KeychainSealer::new("acme-keychain-sealer-test");
        let seed = MasterSeed::from_bytes([0x24u8; 64]);
        let sealed = sealer.seal(&seed).await.expect("seal");
        let recovered = sealer.unseal(&sealed).await.expect("unseal");
        assert_eq!(recovered.expose(), seed.expose());
    }

    #[tokio::test]
    async fn rejects_a_salted_envelope_without_touching_the_keychain() {
        let salted = SealedSeed::new(vec![1, 2, 3, 4], [0u8; 12], vec![0xAB; 80]);
        let err = KeychainSealer::new("unused")
            .unseal(&salted)
            .await
            .expect_err("salted envelope is not ours");
        assert!(matches!(err, KeyError::Unseal));
    }
}
