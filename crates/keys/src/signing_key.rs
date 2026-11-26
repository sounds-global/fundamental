use ed25519_dalek::{Signer, SigningKey as DalekSigningKey};
use fundamental_core::secrets::Secret;

use crate::{KeyError, Signature, VerifyingKey};

/// A device's ed25519 private key, used to sign push requests.
///
/// The seed is a [`Secret`]. `Clone` copies the key; HD derivation reproduces it anyway.
#[derive(Clone)]
pub struct SigningKey {
    inner: Secret<DalekSigningKey>,
}

impl SigningKey {
    /// Generates a fresh keypair from the operating system's CSPRNG.
    ///
    /// # Errors
    /// Returns [`KeyError::Generate`] if the system random source is unavailable.
    pub fn generate() -> Result<Self, KeyError> {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).map_err(|e| KeyError::Generate(e.to_string()))?;
        Ok(Self::from_seed_bytes(seed))
    }

    /// Reconstructs a keypair from a stored 32-byte seed.
    pub fn from_seed(seed: &Secret<[u8; 32]>) -> Self {
        Self::from_seed_bytes(*seed.expose())
    }

    fn from_seed_bytes(seed: [u8; 32]) -> Self {
        Self {
            inner: Secret::new(DalekSigningKey::from_bytes(&seed)),
        }
    }

    /// Returns the 32-byte seed for storage.
    pub fn to_seed(&self) -> Secret<[u8; 32]> {
        Secret::new(self.inner.expose().to_bytes())
    }

    /// Returns the public key that verifies this key's signatures.
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey::from_bytes(self.inner.expose().verifying_key().to_bytes())
    }

    /// Signs `message`.
    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature::from_bytes(self.inner.expose().sign(message).to_bytes())
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn sign_then_verify_round_trips() {
        let key = SigningKey::generate().expect("generate");
        let signature = key.sign(b"hello world");
        key.verifying_key()
            .verify(b"hello world", &signature)
            .expect("valid signature");
    }

    #[test]
    fn verify_rejects_a_tampered_message() {
        let key = SigningKey::generate().expect("generate");
        let signature = key.sign(b"hello world");
        let err = key
            .verifying_key()
            .verify(b"hello WORLD", &signature)
            .expect_err("must reject");
        assert!(matches!(err, KeyError::BadSignature));
        assert_eq!(err.error_code(), "KEYS_002");
        assert_eq!(err.http_status_code(), 401);
    }

    #[test]
    fn verify_rejects_a_different_key() {
        let signing = SigningKey::generate().expect("generate");
        let other = SigningKey::generate().expect("generate");
        let signature = signing.sign(b"msg");
        assert!(matches!(
            other.verifying_key().verify(b"msg", &signature),
            Err(KeyError::BadSignature)
        ));
    }

    #[test]
    fn seed_round_trips_a_keypair() {
        let key = SigningKey::generate().expect("generate");
        let seed = key.to_seed();
        let restored = SigningKey::from_seed(&seed);
        assert_eq!(restored.verifying_key(), key.verifying_key());
        let signature = restored.sign(b"data");
        key.verifying_key()
            .verify(b"data", &signature)
            .expect("seed restored the same key");
    }

    #[test]
    fn secret_seed_redacts_in_debug() {
        let key = SigningKey::generate().expect("generate");
        let seed = key.to_seed();
        assert_eq!(format!("{seed:?}"), "[REDACTED]");
    }
}
