//! Secret wrapper types for sensitive data.
//!
//! # Example
//!
//! ```
//! use fundamental_core::secrets::{Secret, SecretString};
//!
//! let password: SecretString = Secret::new("super-secret".to_string());
//!
//! // Debug output is redacted
//! assert_eq!(format!("{:?}", password), "[REDACTED]");
//!
//! // Access the actual value explicitly
//! assert_eq!(password.expose(), "super-secret");
//! ```

mod database;
mod in_memory_secret;
mod in_memory_string;
mod secret;
mod secret_bytes;
mod secret_json;
mod string;

pub use database::DatabaseUrl;
pub use in_memory_secret::InMemorySecret;
pub use in_memory_string::InMemorySecretString;
pub use secret::Secret;
pub use secret_bytes::SecretBytes;
pub use secret_json::SecretJson;
pub use string::SecretString;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_debug_is_redacted() {
        let secret = Secret::new("my-secret".to_string());
        assert_eq!(format!("{:?}", secret), "[REDACTED]");
    }

    #[test]
    fn test_secret_display_is_redacted() {
        let secret = Secret::new("my-secret".to_string());
        assert_eq!(format!("{}", secret), "[REDACTED]");
    }

    #[test]
    fn test_secret_expose() {
        let secret = Secret::new("my-secret".to_string());
        assert_eq!(secret.expose(), "my-secret");
    }

    #[test]
    fn test_secret_expose_mut() {
        let mut secret = Secret::new("my-secret".to_string());
        secret.expose_mut().push_str("-modified");
        assert_eq!(secret.expose(), "my-secret-modified");
    }

    #[test]
    fn test_secret_expose_owned() {
        let secret = Secret::new("my-secret".to_string());
        let value = secret.expose_owned();
        assert_eq!(value, "my-secret");
    }

    #[test]
    fn test_secret_map() {
        let secret = Secret::new(42i32);
        let doubled = secret.map(|x| x * 2);
        assert_eq!(*doubled.expose(), 84);
    }

    #[test]
    fn test_secret_clone() {
        let secret = Secret::new("my-secret".to_string());
        let cloned = secret.clone();
        assert_eq!(secret.expose(), cloned.expose());
    }

    /// The `==` half is checked by the `compile_fail` doctest on `Secret`.
    #[test]
    fn two_secrets_are_compared_through_expose_and_a_chosen_comparison() {
        let s1 = Secret::new("same".to_string());
        let s2 = Secret::new("same".to_string());
        let s3 = Secret::new("different".to_string());

        assert_eq!(s1.expose(), s2.expose());
        assert_ne!(s1.expose(), s3.expose());
    }

    #[test]
    fn test_secret_default() {
        let secret: Secret<String> = Secret::default();
        assert_eq!(secret.expose(), "");
    }

    #[test]
    fn test_secret_bytes_redacts_and_exposes() {
        let bytes: SecretBytes = Secret::new(vec![0x00, 0x01, 0xff]);
        assert_eq!(format!("{bytes:?}"), "[REDACTED]");
        assert_eq!(format!("{bytes}"), "[REDACTED]");
        assert_eq!(bytes.expose(), &[0x00, 0x01, 0xff]);
    }

    #[test]
    fn test_secret_bytes_serialize_redacts() {
        let bytes: SecretBytes = Secret::new(vec![0xde, 0xad, 0xbe, 0xef]);
        let json = serde_json::to_string(&bytes).expect("serialize");
        assert_eq!(json, "\"[REDACTED]\"");
    }

    #[test]
    fn test_secret_json_redacts_and_exposes() {
        let codes: SecretJson<Vec<String>> =
            Secret::new(vec!["a1b2".to_string(), "c3d4".to_string()]);
        assert_eq!(format!("{codes:?}"), "[REDACTED]");
        assert_eq!(codes.expose().len(), 2);
        assert_eq!(codes.expose()[0], "a1b2");
    }

    #[test]
    fn test_secret_json_serialize_redacts() {
        let codes: SecretJson<Vec<String>> = Secret::new(vec!["topsecret".to_string()]);
        let json = serde_json::to_string(&codes).expect("serialize");
        assert_eq!(json, "\"[REDACTED]\"");
        assert!(!json.contains("topsecret"));
    }

    #[test]
    fn test_database_url_masked() {
        let url = DatabaseUrl::new("postgresql://user:secret@localhost:5432/db");
        assert_eq!(url.masked(), "postgresql://user:***@localhost:5432/db");
    }

    #[test]
    fn test_database_url_no_password() {
        let url = DatabaseUrl::new("postgresql://localhost:5432/db");
        assert_eq!(url.masked(), "postgresql://localhost:5432/db");
    }

    #[test]
    fn test_database_url_no_credentials() {
        let url = DatabaseUrl::new("postgresql://user@localhost:5432/db");
        assert_eq!(url.masked(), "postgresql://user@localhost:5432/db");
    }

    #[test]
    fn test_database_url_expose() {
        let url = DatabaseUrl::new("postgresql://user:secret@localhost:5432/db");
        assert_eq!(url.expose(), "postgresql://user:secret@localhost:5432/db");
    }

    #[test]
    fn test_database_url_debug() {
        let url = DatabaseUrl::new("postgresql://user:secret@localhost:5432/db");
        let debug = format!("{:?}", url);
        assert!(debug.contains("***"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn test_database_url_display() {
        let url = DatabaseUrl::new("postgresql://user:secret@localhost:5432/db");
        let display = format!("{}", url);
        assert!(display.contains("***"));
        assert!(!display.contains("secret"));
    }

    #[test]
    fn test_database_url_with_options() {
        let url = DatabaseUrl::new("postgresql://user:secret@localhost:5432/db?sslmode=require");
        assert_eq!(
            url.masked(),
            "postgresql://user:***@localhost:5432/db?sslmode=require"
        );
    }
}
