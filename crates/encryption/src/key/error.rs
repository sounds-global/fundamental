use fundamental_core::outbox::RefusalKind;
use fundamental_macros::Error;
use fundamental_secrets::SecretError;

/// A key management failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KeyError {
    /// The requested key was not found.
    #[error("key not found: {0}")]
    KeyNotFound(String),

    /// The requested key version was not found.
    #[error("key version {version} not found for key: {key_id}")]
    KeyVersionNotFound {
        /// The key identifier.
        key_id: String,
        /// The requested version.
        version: u32,
    },

    /// A cryptographic operation failed.
    #[error("encryption error: {0}")]
    Encryption(String),

    /// A decryption operation failed.
    #[error("decryption error: {0}")]
    Decryption(String),

    /// Key configuration is invalid.
    #[error("invalid key configuration: {0}")]
    InvalidConfig(String),

    /// KMS provider error (generic, message-only).
    #[error("KMS error: {0}")]
    Kms(String),

    /// A Vault Transit (KMS) operation failed.
    #[error("Vault KMS operation failed")]
    VaultKms(#[from] SecretError),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl KeyError {
    /// The reason a stored row refuses to decode, or `None` when the failure is not the row's:
    /// a key-store outage, a failed seal or a configuration fault.
    #[must_use]
    pub fn refusal(&self) -> Option<RefusalKind> {
        // Exhaustive so a new variant must be decided as a refusal or not.
        match self {
            Self::KeyNotFound(_) | Self::KeyVersionNotFound { .. } => Some(RefusalKind::KeyNotHeld),
            Self::Decryption(_) => Some(RefusalKind::DecryptRefused),
            Self::Serialization(_) => Some(RefusalKind::EnvelopeRefused),
            Self::Encryption(_) | Self::InvalidConfig(_) | Self::Kms(_) | Self::VaultKms(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn vault_kms_preserves_the_secret_error_source() {
        let secret_err = SecretError::Auth("permission denied".to_string());
        let err: KeyError = secret_err.into();
        assert!(matches!(err, KeyError::VaultKms(_)));
        let source = err.source().expect("VaultKms must expose its source");
        assert!(source.to_string().contains("permission denied"));
    }

    #[test]
    fn a_missing_key_or_version_is_a_key_the_reader_does_not_hold() {
        assert_eq!(
            KeyError::KeyNotFound("pii".into()).refusal(),
            Some(RefusalKind::KeyNotHeld)
        );
        let version = KeyError::KeyVersionNotFound {
            key_id: "pii".into(),
            version: 3,
        };
        assert_eq!(version.refusal(), Some(RefusalKind::KeyNotHeld));
    }

    #[test]
    fn a_failed_authentication_and_a_broken_envelope_are_refusals_of_the_row() {
        assert_eq!(
            KeyError::Decryption("tag".into()).refusal(),
            Some(RefusalKind::DecryptRefused)
        );
        assert_eq!(
            KeyError::Serialization("b64".into()).refusal(),
            Some(RefusalKind::EnvelopeRefused)
        );
    }

    #[test]
    fn a_key_store_outage_is_not_a_refusal() {
        assert_eq!(KeyError::Kms("unreachable".into()).refusal(), None);
        assert_eq!(KeyError::InvalidConfig("no key".into()).refusal(), None);
        assert_eq!(KeyError::Encryption("rng".into()).refusal(), None);
        let vault: KeyError = SecretError::Auth("denied".into()).into();
        assert_eq!(vault.refusal(), None);
    }

    #[test]
    fn generic_kms_remains_string_only() {
        let err = KeyError::Kms("no vault client provided".to_string());
        assert!(err.source().is_none());
        assert!(format!("{err}").contains("no vault client provided"));
    }
}
