use super::EncryptionAlgorithm;

/// How a value was encrypted, stored beside its ciphertext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionMetadata {
    /// The identifier of the key used for encryption.
    pub key_id: String,

    /// The version of the key used for encryption.
    pub key_version: u32,

    /// The encryption algorithm used.
    pub algorithm: EncryptionAlgorithm,

    /// The initialization vector (nonce) used for encryption.
    pub iv: Vec<u8>,

    /// The envelope format. [`ROW_BOUND_FORMAT`](crate::ROW_BOUND_FORMAT) for every value sealed
    /// to its row; an envelope written before row binding reads as `0` and is refused.
    pub format: u8,
}

impl EncryptionMetadata {
    /// Creates new encryption metadata.
    pub fn new(
        key_id: impl Into<String>,
        key_version: u32,
        algorithm: EncryptionAlgorithm,
        iv: Vec<u8>,
    ) -> Self {
        Self {
            key_id: key_id.into(),
            key_version,
            algorithm,
            iv,
            format: crate::ROW_BOUND_FORMAT,
        }
    }
}
