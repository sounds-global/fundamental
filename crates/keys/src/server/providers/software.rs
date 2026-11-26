use tracing::instrument;

use crate::server::{KeyId, KeyProtection, Signer};
use crate::{KeyError, Signature, SigningKey, VerifyingKey};

/// A [`Signer`] whose private key lives in process memory.
///
/// [`KeyProtection::Software`], never accreditable: for development, tests and fixtures.
pub struct SoftwareSigner {
    key_id: KeyId,
    key: SigningKey,
}

impl SoftwareSigner {
    /// Wraps an existing [`SigningKey`] under the logical identifier `key_id`.
    pub fn new(key_id: impl Into<KeyId>, key: SigningKey) -> Self {
        Self {
            key_id: key_id.into(),
            key,
        }
    }

    /// Generates a fresh keypair from the OS CSPRNG under the identifier `key_id`.
    ///
    /// # Errors
    /// Returns [`KeyError::Generate`] if the system random source is unavailable.
    pub fn generate(key_id: impl Into<KeyId>) -> Result<Self, KeyError> {
        Ok(Self::new(key_id, SigningKey::generate()?))
    }
}

impl Signer for SoftwareSigner {
    fn key_id(&self) -> KeyId {
        self.key_id.clone()
    }

    fn public_key(&self) -> VerifyingKey {
        self.key.verifying_key()
    }

    fn protection(&self) -> KeyProtection {
        KeyProtection::Software
    }

    #[instrument(name = "keys.software_sign", skip_all, fields(key_id = %self.key_id))]
    fn sign(&self, message: &[u8]) -> Result<Signature, KeyError> {
        Ok(self.key.sign(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_a_verifiable_signature_and_reports_software_protection() {
        let signer = SoftwareSigner::generate("issuer.dev").expect("generate");
        assert_eq!(signer.key_id().as_str(), "issuer.dev");
        assert_eq!(signer.protection(), KeyProtection::Software);
        assert!(!signer.protection().is_accreditable());
        assert!(!signer.dual_control());

        let message = b"statement to sign";
        let signature = signer.sign(message).expect("sign");
        signer
            .public_key()
            .verify(message, &signature)
            .expect("signature verifies under the reported public key");
    }

    #[test]
    fn rejects_a_signature_over_a_different_message() {
        let signer = SoftwareSigner::generate("issuer.dev").expect("generate");
        let signature = signer.sign(b"one message").expect("sign");
        assert!(matches!(
            signer.public_key().verify(b"another message", &signature),
            Err(KeyError::BadSignature)
        ));
    }
}
