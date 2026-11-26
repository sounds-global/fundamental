mod config;
mod entry;
mod error;
mod manager;
mod manager_builder;
mod providers;
mod result;
mod shared_manager;
mod source;
mod version;

pub use config::KeyConfig;
pub use error::KeyError;
pub use manager::KeyManager;
pub use manager_builder::KeyManagerBuilder;
pub use result::KeyResult;
pub use shared_manager::SharedKeyManager;

pub use providers::*;
pub use source::*;

pub(crate) use entry::KeyEntry;
pub(crate) use version::KeyVersion;

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use crate::EncryptionAlgorithm;

    use super::*;
    use crate::RowBinding;

    fn bound_parts() -> Vec<(&'static str, Option<Vec<u8>>)> {
        vec![
            ("zone", Some(b"\x01zone-eu".to_vec())),
            ("id", Some(b"\x02row-0000000001".to_vec())),
        ]
    }

    fn test_key() -> [u8; 32] {
        [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ]
    }

    #[test]
    fn test_encrypt_decrypt_string() {
        let parts = bound_parts();
        let row = RowBinding::new("secrets", "value", &parts);
        let manager = KeyManager::builder()
            .with_local_key("test", &test_key())
            .build()
            .unwrap();

        let plaintext = "Hello, World!";
        let encrypted = manager.encrypt_string(plaintext, "test", &row).unwrap();
        let decrypted = manager.decrypt_string(&encrypted, &row).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_bytes() {
        let parts = bound_parts();
        let row = RowBinding::new("secrets", "value", &parts);
        let manager = KeyManager::builder()
            .with_local_key("test", &test_key())
            .build()
            .unwrap();

        let plaintext = b"binary data \x00\x01\x02";
        let encrypted = manager.encrypt_bytes(plaintext, "test", &row).unwrap();
        let decrypted = manager.decrypt_bytes(&encrypted, &row).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_json_to_string() {
        let parts = bound_parts();
        let row = RowBinding::new("secrets", "value", &parts);
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct TestData {
            name: String,
            value: i32,
        }

        let manager = KeyManager::builder()
            .with_local_key("test", &test_key())
            .build()
            .unwrap();

        let original = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let encrypted = manager
            .encrypt_json_to_string(&original, "test", &row)
            .unwrap();
        let decrypted: TestData = manager.decrypt_string_to_json(&encrypted, &row).unwrap();

        assert_eq!(decrypted, original);
    }

    #[test]
    fn test_key_not_found() {
        let parts = bound_parts();
        let row = RowBinding::new("secrets", "value", &parts);
        let manager = KeyManager::builder()
            .with_local_key("test", &test_key())
            .build()
            .unwrap();

        let result = manager.encrypt_string("test", "nonexistent", &row);
        assert!(matches!(result, Err(KeyError::KeyNotFound(_))));
    }

    #[test]
    fn test_multiple_keys() {
        let parts = bound_parts();
        let row = RowBinding::new("secrets", "value", &parts);
        let key1 = test_key();
        let mut key2 = test_key();
        key2[0] = 0xff; // Different key

        let manager = KeyManager::builder()
            .with_local_key("key1", &key1)
            .with_local_key("key2", &key2)
            .build()
            .unwrap();

        let encrypted1 = manager.encrypt_string("test1", "key1", &row).unwrap();
        let encrypted2 = manager.encrypt_string("test2", "key2", &row).unwrap();

        assert_eq!(manager.decrypt_string(&encrypted1, &row).unwrap(), "test1");
        assert_eq!(manager.decrypt_string(&encrypted2, &row).unwrap(), "test2");

        assert_eq!(encrypted1.key_id(), "key1");
        assert_eq!(encrypted2.key_id(), "key2");
    }

    #[test]
    fn test_key_versioning() {
        let parts = bound_parts();
        let row = RowBinding::new("secrets", "value", &parts);
        let key_v1 = test_key();
        let mut key_v2 = test_key();
        key_v2[0] = 0xff;

        let manager = KeyManager::builder()
            .with_key_version("test", 1, &key_v1, EncryptionAlgorithm::Aes256Gcm)
            .with_key_version("test", 2, &key_v2, EncryptionAlgorithm::Aes256Gcm)
            .build()
            .unwrap();

        let encrypted = manager.encrypt_string("test", "test", &row).unwrap();
        assert_eq!(encrypted.key_version(), 2);

        let decrypted = manager.decrypt_string(&encrypted, &row).unwrap();
        assert_eq!(decrypted, "test");
    }

    #[test]
    fn test_chacha20() {
        let parts = bound_parts();
        let row = RowBinding::new("secrets", "value", &parts);
        let manager = KeyManager::builder()
            .default_algorithm(EncryptionAlgorithm::ChaCha20Poly1305)
            .with_local_key("test", &test_key())
            .build()
            .unwrap();

        let plaintext = "Hello, ChaCha20!";
        let encrypted = manager.encrypt_string(plaintext, "test", &row).unwrap();
        let decrypted = manager.decrypt_string(&encrypted, &row).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn two_keys_with_identical_material_are_refused() {
        let result = KeyManager::builder()
            .with_local_key("pii", &test_key())
            .with_local_key("mail", &test_key())
            .build();
        let Err(KeyError::InvalidConfig(problem)) = result else {
            panic!("identical material must be refused");
        };
        assert!(
            problem.contains("'mail'") && problem.contains("'pii'"),
            "{problem}"
        );
    }

    #[test]
    fn a_ciphertext_moved_to_another_row_refuses_to_decrypt() {
        let manager = KeyManager::builder()
            .with_local_key("pii", &test_key())
            .build()
            .unwrap();
        let parts = bound_parts();
        let sealed = manager
            .encrypt_string(
                "person A",
                "pii",
                &RowBinding::new("secrets", "value", &parts),
            )
            .unwrap();

        let other_row = vec![
            ("zone", Some(b"\x01zone-eu".to_vec())),
            ("id", Some(b"\x02row-0000000002".to_vec())),
        ];
        for elsewhere in [
            RowBinding::new("secrets", "value", &other_row),
            RowBinding::new("secrets", "salt", &parts),
            RowBinding::new("other_secrets", "value", &parts),
        ] {
            let error = manager
                .decrypt_string(&sealed, &elsewhere)
                .expect_err("a moved ciphertext must not open");
            assert!(matches!(error, KeyError::Decryption(_)), "{error}");
        }
    }

    #[test]
    fn a_value_whose_bound_column_changed_refuses_to_decrypt() {
        let manager = KeyManager::builder()
            .with_local_key("pii", &test_key())
            .build()
            .unwrap();
        let written = vec![
            ("offer_id", Some(b"\x01offer-a".to_vec())),
            (
                "confirmed_at",
                Some(b"\x06\x10\x00\x00\x00\x00\x00\x00\x00".to_vec()),
            ),
        ];
        let sealed = manager
            .encrypt_string(
                "person A",
                "pii",
                &RowBinding::new("staged", "value", &written),
            )
            .unwrap();
        let restamped = vec![
            ("offer_id", Some(b"\x01offer-a".to_vec())),
            (
                "confirmed_at",
                Some(b"\x06\x20\x00\x00\x00\x00\x00\x00\x00".to_vec()),
            ),
        ];
        let moved = vec![
            ("offer_id", Some(b"\x01offer-b".to_vec())),
            (
                "confirmed_at",
                Some(b"\x06\x10\x00\x00\x00\x00\x00\x00\x00".to_vec()),
            ),
        ];
        for edited in [&restamped, &moved] {
            let error = manager
                .decrypt_string(&sealed, &RowBinding::new("staged", "value", edited))
                .expect_err("an edited row must not open");
            assert!(matches!(error, KeyError::Decryption(_)), "{error}");
        }
        assert_eq!(
            manager
                .decrypt_string(&sealed, &RowBinding::new("staged", "value", &written))
                .unwrap(),
            "person A"
        );
    }

    #[test]
    fn an_envelope_from_before_row_binding_is_refused() {
        let manager = KeyManager::builder()
            .with_local_key("pii", &test_key())
            .build()
            .unwrap();
        let parts = bound_parts();
        let row = RowBinding::new("secrets", "value", &parts);
        let mut sealed = manager.encrypt_string("person A", "pii", &row).unwrap();
        sealed.metadata.format = 0;
        let Err(KeyError::Decryption(problem)) = manager.decrypt_string(&sealed, &row) else {
            panic!("an unbound envelope must be refused");
        };
        assert!(problem.contains("format 0"), "{problem}");
    }

    #[test]
    fn a_key_declared_for_two_purposes_is_refused() {
        let mut other = test_key();
        other[0] = 0xff;
        let mut builder = KeyManager::builder();
        builder.add_key_version(
            "pii",
            1,
            &test_key(),
            EncryptionAlgorithm::Aes256Gcm,
            "one.v1",
        );
        builder.add_key_version("pii", 2, &other, EncryptionAlgorithm::Aes256Gcm, "two.v1");
        let Err(KeyError::InvalidConfig(problem)) = builder.build() else {
            panic!("conflicting purposes must be refused");
        };
        assert!(
            problem.contains("one.v1") && problem.contains("two.v1"),
            "{problem}"
        );
    }

    #[test]
    fn test_builder_requires_key() {
        let result = KeyManager::builder().build();
        assert!(matches!(result, Err(KeyError::InvalidConfig(_))));
    }
}
