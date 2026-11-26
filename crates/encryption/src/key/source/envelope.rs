use tracing::{debug, instrument};

use super::matches_declared_fingerprint::matches_declared_fingerprint;
use crate::{EncryptedKeyConfig, KeyConfig, KeyError, KeyResult, KeySource, KmsProvider};

/// A key source that uses envelope encryption with a KMS provider.
///
/// Each [`EncryptedKeyConfig::encrypted_dek`] is base64 ciphertext unwrapped on load by the
/// [`KmsProvider`], such as [`VaultKmsProvider`](crate::VaultKmsProvider).
///
/// # Examples
///
/// ```ignore
/// use fundamental_encryption::{
///     EncryptedKeyConfig, EncryptionAlgorithm, EnvelopeKeySource, KeyManagerBuilder,
///     VaultKmsProvider, DEFAULT_TRANSIT_MOUNT,
/// };
///
/// // In production, load these from your secrets manager or config
/// let encrypted_keys = vec![
///     EncryptedKeyConfig {
///         id: "default".to_string(),
///         encrypted_dek: std::env::var("ENCRYPTED_DEK_DEFAULT")?,
///         kms_key_id: "fundamental-default".to_string(),
///         version: 1,
///         algorithm: EncryptionAlgorithm::Aes256Gcm,
///         // Refused at load in production: declare what the unwrapped key must be.
///         fingerprint: None,
///     },
///     EncryptedKeyConfig {
///         id: "pii".to_string(),
///         encrypted_dek: std::env::var("ENCRYPTED_DEK_PII")?,
///         kms_key_id: "fundamental-pii".to_string(),
///         version: 1,
///         algorithm: EncryptionAlgorithm::Aes256Gcm,
///         // What the unwrapped key must be; a DEK swapped behind this config is refused.
///         fingerprint: Some(std::env::var("FINGERPRINT_PII")?),
///     },
/// ];
///
/// // `vault_client` is a configured `fundamental_secrets::VaultClient`.
/// let kms = VaultKmsProvider::new(vault_client, DEFAULT_TRANSIT_MOUNT);
/// let source = EnvelopeKeySource::new(Box::new(kms), encrypted_keys);
/// let key_manager = KeyManagerBuilder::from_source(&source).await?;
/// ```
pub struct EnvelopeKeySource {
    kms: Box<dyn KmsProvider>,
    encrypted_keys: Vec<EncryptedKeyConfig>,
}

impl EnvelopeKeySource {
    /// Creates a new envelope key source.
    pub fn new(kms: Box<dyn KmsProvider>, encrypted_keys: Vec<EncryptedKeyConfig>) -> Self {
        Self {
            kms,
            encrypted_keys,
        }
    }
}

impl std::fmt::Debug for EnvelopeKeySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvelopeKeySource")
            .field("encrypted_keys", &self.encrypted_keys.len())
            .finish()
    }
}

#[async_trait::async_trait]
impl KeySource for EnvelopeKeySource {
    #[instrument(skip(self), fields(keys = self.encrypted_keys.len()), err(Debug))]
    async fn load_keys(&self) -> KeyResult<Vec<KeyConfig>> {
        use base64::{Engine, engine::general_purpose::STANDARD};

        let mut configs = Vec::with_capacity(self.encrypted_keys.len());

        for encrypted in &self.encrypted_keys {
            debug!(key_id = %encrypted.id, kms_key_id = %encrypted.kms_key_id, "unwrapping envelope DEK via KMS");
            let ciphertext = STANDARD.decode(&encrypted.encrypted_dek).map_err(|e| {
                KeyError::InvalidConfig(format!("invalid base64 for key '{}': {}", encrypted.id, e))
            })?;

            let material = self.kms.decrypt(&ciphertext, &encrypted.kms_key_id).await?;

            if material.len() != 32 {
                return Err(KeyError::InvalidConfig(format!(
                    "decrypted key '{}' must be 32 bytes, got {}",
                    encrypted.id,
                    material.len()
                )));
            }

            // A wrapped DEK swapped behind the configuration unwraps cleanly into another key.
            let fingerprint_declared = match &encrypted.fingerprint {
                None => false,
                Some(declared) if matches_declared_fingerprint(declared, &material) => true,
                Some(_) => {
                    return Err(KeyError::InvalidConfig(format!(
                        "key '{}' version {}: the unwrapped material does not match the fingerprint declared for it",
                        encrypted.id, encrypted.version
                    )));
                }
            };

            configs.push(KeyConfig {
                id: encrypted.id.clone(),
                material,
                version: encrypted.version,
                algorithm: encrypted.algorithm,
                purpose: String::new(),
                fingerprint_declared,
            });
        }

        Ok(configs)
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::STANDARD};

    use super::*;
    use crate::EncryptionAlgorithm;
    use crate::fns::material_fingerprint;

    /// A KMS whose ciphertext is the key itself.
    struct Unwrapping;

    #[async_trait::async_trait]
    impl KmsProvider for Unwrapping {
        async fn decrypt(&self, ciphertext: &[u8], _key_id: &str) -> KeyResult<Vec<u8>> {
            Ok(ciphertext.to_vec())
        }

        async fn encrypt(&self, plaintext: &[u8], _key_id: &str) -> KeyResult<Vec<u8>> {
            Ok(plaintext.to_vec())
        }
    }

    fn wrapped(material: u8, fingerprint: Option<String>) -> EncryptedKeyConfig {
        EncryptedKeyConfig {
            id: "pii".to_owned(),
            encrypted_dek: STANDARD.encode([material; 32]),
            kms_key_id: "transit-pii".to_owned(),
            version: 1,
            algorithm: EncryptionAlgorithm::ChaCha20Poly1305,
            fingerprint,
        }
    }

    fn fingerprint_of(material: u8) -> String {
        material_fingerprint(&[material; 32])
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    async fn load(key: EncryptedKeyConfig) -> KeyResult<Vec<KeyConfig>> {
        EnvelopeKeySource::new(Box::new(Unwrapping), vec![key])
            .load_keys()
            .await
    }

    #[tokio::test]
    async fn a_key_matching_its_declared_fingerprint_loads_as_declared() {
        let keys = load(wrapped(0x07, Some(fingerprint_of(0x07))))
            .await
            .expect("the declared key loads");
        assert!(keys[0].fingerprint_declared);
    }

    #[tokio::test]
    async fn a_substituted_key_is_refused_at_load() {
        let refused = load(wrapped(0x08, Some(fingerprint_of(0x07))))
            .await
            .expect_err("material the fleet did not declare is refused");
        assert!(
            matches!(&refused, KeyError::InvalidConfig(reason) if reason.contains("does not match the fingerprint")),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn an_undeclared_key_loads_undeclared() {
        let keys = load(wrapped(0x07, None))
            .await
            .expect("an undeclared key loads");
        assert!(!keys[0].fingerprint_declared);
    }
}
