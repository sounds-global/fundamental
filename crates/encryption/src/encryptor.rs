use super::{EncryptionMetadata, RowBinding};

/// Encrypts and decrypts field values.
pub trait Encryptor: Send + Sync {
    /// The error type for encryption operations.
    type Error;

    /// Encrypts plaintext bytes using the specified key, sealed to `row`.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    fn encrypt_bytes(
        &self,
        key_id: &str,
        plaintext: &[u8],
        row: &RowBinding<'_>,
    ) -> Result<(Vec<u8>, EncryptionMetadata), Self::Error>;

    /// Decrypts ciphertext bytes using the metadata, refusing unless `row` is the binding the
    /// value was sealed to.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    fn decrypt_bytes(
        &self,
        ciphertext: &[u8],
        metadata: &EncryptionMetadata,
        row: &RowBinding<'_>,
    ) -> Result<Vec<u8>, Self::Error>;
}
