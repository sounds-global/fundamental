//! The ciphertext carriers the `#[derive(Model)]` `Secret` codec names; hidden, not public API.

mod bytes;
mod envelope;
mod key_ids;
mod string;

pub use bytes::EncryptedBytes;
pub use envelope::EncryptedEnvelope;
pub use string::EncryptedString;

pub use key_ids::*;

use super::EncryptionMetadata;

/// An encrypted carrier.
#[doc(hidden)]
pub trait Encrypted {
    /// The plaintext type that this encrypted type wraps.
    type Plaintext;

    /// Returns the encryption metadata.
    fn metadata(&self) -> &EncryptionMetadata;

    /// Returns the raw ciphertext bytes.
    fn ciphertext(&self) -> &[u8];

    /// Returns whether the value is currently encrypted.
    fn is_encrypted(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::EncryptionAlgorithm;

    use super::*;

    fn create_test_metadata() -> EncryptionMetadata {
        EncryptionMetadata {
            key_id: "test".to_string(),
            key_version: 1,
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            iv: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            format: crate::ROW_BOUND_FORMAT,
        }
    }

    #[test]
    fn test_encrypted_string_serialization() {
        let encrypted =
            EncryptedString::from_encrypted(vec![1, 2, 3, 4, 5], create_test_metadata());

        let json = serde_json::to_string(&encrypted).unwrap();
        let deserialized: EncryptedString = serde_json::from_str(&json).unwrap();

        assert_eq!(encrypted.ciphertext, deserialized.ciphertext);
        assert_eq!(encrypted.metadata.key_id, deserialized.metadata.key_id);
        assert_eq!(
            encrypted.metadata.key_version,
            deserialized.metadata.key_version
        );
    }

    #[test]
    fn test_encrypted_bytes_serialization() {
        let encrypted =
            EncryptedBytes::from_encrypted(vec![10, 20, 30, 40, 50], create_test_metadata());

        let json = serde_json::to_string(&encrypted).unwrap();
        let deserialized: EncryptedBytes = serde_json::from_str(&json).unwrap();

        assert_eq!(encrypted.ciphertext, deserialized.ciphertext);
    }

    #[test]
    fn test_encrypted_envelope_format() {
        let encrypted = EncryptedString::from_encrypted(vec![1, 2, 3], create_test_metadata());

        let json = serde_json::to_string_pretty(&encrypted).unwrap();

        assert!(json.contains("ciphertext"));
        assert!(json.contains("key_id"));
        assert!(json.contains("key_version"));
        assert!(json.contains("algorithm"));
        assert!(json.contains("iv"));
        assert!(json.contains("\"format\": 2"));
    }

    #[test]
    fn test_key_ids() {
        assert_eq!(key_ids::DEFAULT, "default");
        assert_eq!(key_ids::PII, "pii");
        assert_eq!(key_ids::FINANCIAL, "financial");
    }

    #[test]
    fn test_encrypted_debug_hides_data() {
        let encrypted = EncryptedString::from_encrypted(
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            create_test_metadata(),
        );

        let debug = format!("{:?}", encrypted);

        // Debug never shows the ciphertext.
        assert!(debug.contains("ciphertext_len"));
        assert!(debug.contains("10")); // The length
        assert!(!debug.contains("[1, 2, 3")); // Not the actual bytes
    }
}
