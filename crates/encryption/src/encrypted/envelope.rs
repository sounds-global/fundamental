use serde::{Deserialize, Serialize};

use crate::{EncryptionAlgorithm, EncryptionMetadata};

/// The JSON form of an encrypted carrier; not public API.
#[doc(hidden)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    /// Base64-encoded ciphertext.
    pub ciphertext: String,

    /// The key identifier.
    pub key_id: String,

    /// The key version.
    pub key_version: u32,

    /// The encryption algorithm.
    pub algorithm: String,

    /// Base64-encoded initialization vector.
    pub iv: String,

    /// The envelope format; absent in an envelope written before row binding, which reads as `0`.
    #[serde(default)]
    pub format: u8,
}

impl EncryptedEnvelope {
    /// Creates a new encrypted envelope.
    pub fn new(ciphertext: &[u8], metadata: &EncryptionMetadata) -> Self {
        use base64::{Engine, engine::general_purpose::STANDARD};

        Self {
            ciphertext: STANDARD.encode(ciphertext),
            key_id: metadata.key_id.clone(),
            key_version: metadata.key_version,
            algorithm: match metadata.algorithm {
                EncryptionAlgorithm::Aes256Gcm => "aes-256-gcm".to_string(),
                EncryptionAlgorithm::ChaCha20Poly1305 => "chacha20-poly1305".to_string(),
            },
            iv: STANDARD.encode(&metadata.iv),
            format: metadata.format,
        }
    }

    /// Decodes the ciphertext from base64.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn decode_ciphertext(&self) -> Result<Vec<u8>, base64::DecodeError> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        STANDARD.decode(&self.ciphertext)
    }

    /// Decodes the IV from base64.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn decode_iv(&self) -> Result<Vec<u8>, base64::DecodeError> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        STANDARD.decode(&self.iv)
    }

    /// Converts to encryption metadata.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn to_metadata(&self) -> Result<EncryptionMetadata, &'static str> {
        let algorithm = match self.algorithm.as_str() {
            "aes-256-gcm" => EncryptionAlgorithm::Aes256Gcm,
            "chacha20-poly1305" => EncryptionAlgorithm::ChaCha20Poly1305,
            _ => return Err("unknown encryption algorithm"),
        };

        let iv = self.decode_iv().map_err(|_| "invalid iv encoding")?;

        Ok(EncryptionMetadata {
            key_id: self.key_id.clone(),
            key_version: self.key_version,
            algorithm,
            iv,
            format: self.format,
        })
    }
}
