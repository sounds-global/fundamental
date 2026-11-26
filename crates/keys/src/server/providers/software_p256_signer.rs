use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature as EcdsaSignature, SigningKey as EcdsaSigningKey};
use tracing::warn;

use crate::KeyError;
use crate::server::{
    AsyncSigner, Es256, KeyId, KeyProtection, P256Signature, P256VerifyingKey, SignFuture,
};

/// An [`AsyncSigner<Es256>`](AsyncSigner) whose P-256 key lives in process memory.
///
/// [`KeyProtection::Software`], never accreditable: for development, tests and fixtures.
pub struct SoftwareP256Signer {
    key_id: KeyId,
    key: EcdsaSigningKey,
    public_key: P256VerifyingKey,
}

impl SoftwareP256Signer {
    /// Generates a fresh key from the OS CSPRNG under `key_id`.
    ///
    /// # Errors
    /// Returns [`KeyError::Generate`] if the system random source is unavailable.
    pub fn generate(key_id: impl Into<KeyId>) -> Result<Self, KeyError> {
        // Redraws an out-of-range scalar rather than biasing the key.
        loop {
            let mut bytes = [0u8; 32];
            getrandom::getrandom(&mut bytes)
                .map_err(|error| KeyError::Generate(format!("entropy: {error}")))?;
            if let Ok(key) = EcdsaSigningKey::from_slice(&bytes) {
                return Self::from_signing_key(key_id, key);
            }
            warn!("drew a P-256 scalar outside the group order; redrawing");
        }
    }

    /// Loads a key from a PKCS#8 PEM document.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if the document is not a P-256 PKCS#8 key.
    pub fn from_pkcs8_pem(key_id: impl Into<KeyId>, pem: &str) -> Result<Self, KeyError> {
        use p256::pkcs8::DecodePrivateKey;
        let key = EcdsaSigningKey::from_pkcs8_pem(pem)
            .map_err(|_| KeyError::Malformed("not a P-256 PKCS#8 private key".to_owned()))?;
        Self::from_signing_key(key_id, key)
    }

    fn from_signing_key(key_id: impl Into<KeyId>, key: EcdsaSigningKey) -> Result<Self, KeyError> {
        let public_key =
            P256VerifyingKey::from_sec1(key.verifying_key().to_encoded_point(false).as_bytes())?;
        Ok(Self {
            key_id: key_id.into(),
            key,
            public_key,
        })
    }
}

impl AsyncSigner<Es256> for SoftwareP256Signer {
    fn key_id(&self) -> KeyId {
        self.key_id.clone()
    }

    fn public_key(&self) -> P256VerifyingKey {
        self.public_key.clone()
    }

    fn protection(&self) -> KeyProtection {
        KeyProtection::Software
    }

    fn sign<'a>(&'a self, message: &'a [u8]) -> SignFuture<'a, Es256> {
        let signature: EcdsaSignature = self.key.sign(message);
        let mut raw = [0u8; 64];
        raw.copy_from_slice(&signature.to_bytes());
        Box::pin(std::future::ready(P256Signature::from_bytes(&raw)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::SignatureAlgorithm;

    #[tokio::test]
    async fn signs_and_reports_software_protection() {
        let signer = SoftwareP256Signer::generate("dev").expect("generate");
        assert!(!AsyncSigner::protection(&signer).is_accreditable());
        let message = b"dev statement";
        let signature = AsyncSigner::sign(&signer, message).await.expect("signs");
        Es256::verify(&AsyncSigner::public_key(&signer), message, &signature).expect("verifies");
    }

    #[tokio::test]
    async fn a_tampered_es256_signature_fails_closed() {
        let signer = SoftwareP256Signer::generate("dev").expect("generate");
        let signature = AsyncSigner::sign(&signer, b"one").await.expect("signs");
        assert!(matches!(
            Es256::verify(&AsyncSigner::public_key(&signer), b"two", &signature),
            Err(KeyError::BadSignature)
        ));
    }

    #[test]
    fn a_non_p256_pem_is_refused() {
        assert!(matches!(
            SoftwareP256Signer::from_pkcs8_pem(
                "dev",
                "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n"
            ),
            Err(KeyError::Malformed(_))
        ));
    }
}
