//! Key management for the Fundamental framework's at-rest field encryption.
//!
//! Build a [`KeyManager`] and attach it to a connection registry with `.with_key_manager(...)`;
//! the persistence layer then encrypts and decrypts `Secret` model fields.
//!
//!
//! ```no_run
//! use fundamental_encryption::KeyManager;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let km = KeyManager::builder()
//!     .with_local_key("pii", &[9u8; 32])
//!     .build()?;
//! # let _ = km;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

mod algorithm;
mod encrypted;
mod encryptor;
mod fns;
mod key;
mod keyring;
mod metadata;
mod row_binding;
mod stream;

pub use algorithm::EncryptionAlgorithm;
pub use encryptor::Encryptor;
pub use metadata::EncryptionMetadata;
pub use row_binding::{ROW_BOUND_FORMAT, RowBinding};

pub use encrypted::*;
pub use fns::*;
pub use key::*;
pub use keyring::*;
pub use stream::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_metadata() {
        let metadata = EncryptionMetadata::new(
            "pii",
            1,
            EncryptionAlgorithm::Aes256Gcm,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        );

        assert_eq!(metadata.key_id, "pii");
        assert_eq!(metadata.key_version, 1);
        assert_eq!(metadata.algorithm, EncryptionAlgorithm::Aes256Gcm);
        assert_eq!(metadata.iv.len(), 12);
        assert_eq!(metadata.format, ROW_BOUND_FORMAT);
    }

    #[test]
    fn test_encryption_algorithm_default() {
        assert_eq!(
            EncryptionAlgorithm::default(),
            EncryptionAlgorithm::Aes256Gcm
        );
    }
}
