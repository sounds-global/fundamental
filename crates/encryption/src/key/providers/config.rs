use crate::EncryptionAlgorithm;

/// Configuration for a key that is encrypted by KMS.
#[derive(Debug, Clone)]
pub struct EncryptedKeyConfig {
    /// The key identifier used in your application (e.g., "pii", "default").
    pub id: String,
    /// The encrypted DEK (base64 encoded).
    pub encrypted_dek: String,
    /// The KMS key ID/ARN used to encrypt this DEK.
    pub kms_key_id: String,
    /// The key version.
    pub version: u32,
    /// The encryption algorithm.
    pub algorithm: EncryptionAlgorithm,
    /// The fingerprint the unwrapped material must have, as 64 hex characters, when declared.
    pub fingerprint: Option<String>,
}
