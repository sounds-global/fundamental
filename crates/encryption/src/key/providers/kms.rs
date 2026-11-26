use crate::KeyResult;

/// A KMS that wraps and unwraps data encryption keys.
/// # Example Implementation
///
/// ```ignore
/// use fundamental_encryption::{KmsProvider, KeyResult};
///
/// struct AwsKms {
///     client: aws_sdk_kms::Client,
/// }
///
/// #[async_trait::async_trait]
/// impl KmsProvider for AwsKms {
///     async fn decrypt(&self, ciphertext: &[u8], key_arn: &str) -> KeyResult<Vec<u8>> {
///         let response = self.client
///             .decrypt()
///             .key_id(key_arn)
///             .ciphertext_blob(Blob::new(ciphertext))
///             .send()
///             .await?;
///         Ok(response.plaintext.unwrap().into_inner())
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait KmsProvider: Send + Sync {
    /// Decrypts ciphertext using the KMS key.
    async fn decrypt(&self, ciphertext: &[u8], key_id: &str) -> KeyResult<Vec<u8>>;

    /// Encrypts plaintext using the KMS key (for key generation/rotation).
    async fn encrypt(&self, plaintext: &[u8], key_id: &str) -> KeyResult<Vec<u8>>;
}
