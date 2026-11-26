//! Error types for the secret-manager (HashiCorp Vault) backend.

use fundamental_macros::Error;

use crate::StoreAnswer;

/// Errors raised by the secret-manager (HashiCorp Vault) backend.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SecretError {
    /// The Vault backend is required but was not provided/enabled.
    #[error(
        code = "SECRETS_001",
        message = "vault backend not configured: {0}",
        status = 500
    )]
    NotConfigured(String),

    /// Authentication to Vault failed.
    #[error(
        code = "SECRETS_002",
        message = "vault authentication failed: {0}",
        status = 502
    )]
    Auth(String),

    /// The HTTP transport failed (connection, TLS, timeout).
    #[error(
        code = "SECRETS_003",
        message = "vault request transport failed",
        status = 503,
        retryable
    )]
    Transport(#[from] reqwest::Error),

    /// Vault returned a non-success status for a request.
    #[error(
        code = "SECRETS_004",
        message = "vault API error ({status}) for {path}: {message}",
        status = 502
    )]
    Api {
        /// HTTP status code returned by Vault.
        status: u16,
        /// What the status amounts to; a `5xx` or `429` is retryable.
        #[overrides]
        answer: StoreAnswer,
        /// The logical Vault path requested.
        path: String,
        /// Error message(s) reported by Vault.
        message: String,
    },

    /// A Vault response did not contain the expected field/shape.
    #[error(
        code = "SECRETS_005",
        message = "unexpected vault response for {path}: {message}",
        status = 502
    )]
    UnexpectedResponse {
        /// The logical Vault path requested.
        path: String,
        /// What was missing or malformed.
        message: String,
    },

    /// Local IO error (e.g. reading a Kubernetes service-account token / CA cert).
    #[error(
        code = "SECRETS_006",
        message = "secrets IO error: {context}",
        status = 500
    )]
    Io {
        /// What was being read when the failure occurred (e.g. the file path).
        context: String,
        /// The underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// Invalid configuration supplied to the secrets layer.
    #[error(
        code = "SECRETS_007",
        message = "invalid secrets configuration: {0}",
        status = 500
    )]
    InvalidConfig(String),
    /// A secret exists at the path but holds no string field of the name asked for.
    #[error(
        code = "SECRETS_008",
        message = "the secret at {path} holds no string field `{key}`",
        status = 500
    )]
    MissingField {
        /// The logical path read.
        path: String,
        /// The field asked for.
        key: String,
    },

    /// A key-value write's check-and-set did not match the secret's current version.
    #[error(
        code = "SECRETS_009",
        message = "the secret at {path} has moved past the version this write expected",
        status = 409
    )]
    CasMismatch {
        /// The logical path written.
        path: String,
    },

    /// The version of a key-value secret asked for is not there: never written, deleted or destroyed.
    #[error(
        code = "SECRETS_010",
        message = "version {version} of the secret at {path} is gone",
        status = 410
    )]
    VersionGone {
        /// The logical path read.
        path: String,
        /// The version asked for.
        version: u64,
    },

    /// A key-value write would exceed the body limit; refused before a byte was sent.
    #[error(
        code = "SECRETS_011",
        message = "a write to {path} exceeds {limit} bytes",
        status = 413
    )]
    WriteTooLarge {
        /// The logical path that would have been written.
        path: String,
        /// The limit, in bytes.
        limit: usize,
    },
}

impl SecretError {
    /// The store's refusal of a request to `path` with HTTP `status`; a `5xx` or `429` is retryable.
    #[must_use]
    pub fn api(status: u16, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Api {
            status,
            answer: StoreAnswer::of_status(status),
            path: path.into(),
            message: message.into(),
        }
    }
}

/// Result alias for the secrets layer.
pub type SecretResult<T> = Result<T, SecretError>;

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn every_variant_carries_a_stable_code() {
        let api = SecretError::api(503, "p", "m");
        assert_eq!(api.error_code(), "SECRETS_004");
        assert_eq!(
            SecretError::Auth("x".to_owned()).error_code(),
            "SECRETS_002"
        );
        assert_eq!(
            SecretError::InvalidConfig("x".to_owned()).error_code(),
            "SECRETS_007"
        );
        let missing = SecretError::MissingField {
            path: "secret/data/nats/app".to_owned(),
            key: "password".to_owned(),
        };
        assert_eq!(missing.error_code(), "SECRETS_008");
        assert!(!missing.is_retryable());
    }

    #[test]
    fn the_key_value_write_refusals_carry_their_codes_and_statuses() {
        let cas = SecretError::CasMismatch {
            path: "kv/data/a".to_owned(),
        };
        assert_eq!(
            (cas.error_code(), cas.http_status_code(), cas.is_retryable()),
            ("SECRETS_009", 409, false)
        );
        let gone = SecretError::VersionGone {
            path: "kv/data/a".to_owned(),
            version: 3,
        };
        assert_eq!(
            (gone.error_code(), gone.http_status_code()),
            ("SECRETS_010", 410)
        );
        let large = SecretError::WriteTooLarge {
            path: "kv/data/a".to_owned(),
            limit: 10,
        };
        assert_eq!(
            (large.error_code(), large.http_status_code()),
            ("SECRETS_011", 413)
        );
    }
}
