use fundamental_macros::Error;
use fundamental_secrets::SecretError;

use crate::StoreAnswer;

/// Result alias for fallible NATS operations.
pub type NatsResult<T> = Result<T, NatsError>;

/// An error from a NATS operation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum NatsError {
    /// Failed to establish or maintain a connection to the server.
    #[error(
        code = "NATS_001",
        message = "Connection error: {0}",
        status = 503,
        retryable
    )]
    Connection(String),

    /// The connection was closed and will not be reused.
    #[error(
        code = "NATS_002",
        message = "Connection closed: {reason}",
        status = 503,
        retryable
    )]
    ConnectionClosed {
        /// Why the connection closed.
        reason: String,
    },

    /// Authentication with the server failed.
    #[error(
        code = "NATS_003",
        message = "Authentication failed: {0}",
        status = 401
    )]
    Authentication(String),

    /// A subscription could not be created or maintained.
    #[error(code = "NATS_004", message = "Subscription error: {0}", status = 500)]
    Subscription(String),

    /// A message could not be published.
    #[error(code = "NATS_005", message = "Publish error: {0}", status = 500)]
    Publish(String),

    /// A request did not receive a reply within the timeout.
    #[error(
        code = "NATS_006",
        message = "Request timed out after {timeout_ms}ms",
        status = 504,
        retryable
    )]
    Timeout {
        /// The elapsed timeout in milliseconds.
        timeout_ms: u64,
    },

    /// No subscribers are listening on the request subject.
    #[error(
        code = "NATS_007",
        message = "No responders for subject: {subject}",
        status = 503,
        retryable
    )]
    NoResponders {
        /// The subject that had no responders.
        subject: String,
    },

    /// A message payload could not be (de)serialized.
    #[error(code = "NATS_008", message = "Serialization error: {0}", status = 500)]
    Serialization(#[from] serde_json::Error),

    /// A subject string failed validation.
    #[error(
        code = "NATS_009",
        message = "Invalid subject {subject}: {reason}",
        status = 400
    )]
    InvalidSubject {
        /// The offending subject.
        subject: String,
        /// Why it is invalid.
        reason: String,
    },

    /// A payload exceeded the configured maximum size.
    #[error(
        code = "NATS_010",
        message = "Message too large: {size} bytes (max {max_size})",
        status = 413
    )]
    MessageTooLarge {
        /// The payload size in bytes.
        size: usize,
        /// The configured maximum in bytes.
        max_size: usize,
    },

    /// A JetStream operation failed.
    #[cfg(feature = "jetstream")]
    #[error(code = "NATS_011", message = "JetStream error: {0}", status = 500)]
    JetStream(String),

    /// The named stream does not exist.
    #[cfg(feature = "jetstream")]
    #[error(code = "NATS_012", message = "Stream not found: {0}", status = 404)]
    StreamNotFound(String),

    /// The named consumer does not exist.
    #[cfg(feature = "jetstream")]
    #[error(code = "NATS_013", message = "Consumer not found: {0}", status = 404)]
    ConsumerNotFound(String),

    /// A stream lookup did not complete, so whether the stream exists is not known; never read
    /// it as absent.
    #[cfg(feature = "jetstream")]
    #[error(
        code = "NATS_020",
        message = "The stream {stream} could not be looked up: {reason}",
        status = 503,
        retryable
    )]
    StreamLookupFailed {
        /// The stream asked for.
        stream: String,
        /// Why the request did not complete.
        reason: String,
    },

    /// A consumer lookup did not complete, so whether the consumer exists is not known.
    #[cfg(feature = "jetstream")]
    #[error(
        code = "NATS_021",
        message = "The consumer {consumer} could not be looked up: {reason}",
        status = 503,
        retryable
    )]
    ConsumerLookupFailed {
        /// The consumer asked for.
        consumer: String,
        /// Why the request did not complete.
        reason: String,
    },

    /// A stream exists under the declared name with a different configuration.
    ///
    /// Refused, never updated: an update can shrink retention into a purge.
    #[cfg(feature = "jetstream")]
    #[error(
        code = "NATS_022",
        message = "The stream {stream} exists with a different configuration: {fields}",
        status = 409
    )]
    StreamDrift {
        /// The stream.
        stream: String,
        /// Every field that differs, comma-separated.
        fields: String,
    },

    /// A message acknowledgment failed.
    #[cfg(feature = "jetstream")]
    #[error(
        code = "NATS_014",
        message = "Acknowledgment failed: {0}",
        status = 500
    )]
    AckFailed(String),

    /// The client configuration is invalid.
    #[error(code = "NATS_015", message = "Configuration error: {0}", status = 500)]
    Config(String),

    /// An unexpected internal error.
    #[error(code = "NATS_016", message = "Internal error: {0}", status = 500)]
    Internal(String),

    /// Failed to register or await the OS shutdown signal.
    #[error(
        code = "NATS_017",
        message = "Failed to listen for shutdown signal: {source}",
        status = 500
    )]
    Signal {
        /// The underlying IO error from the signal handler.
        #[source]
        source: std::io::Error,
    },

    /// A message-set delivery arrived on a subject no variant claims; a bug, not retryable.
    #[error(
        code = "NATS_018",
        message = "No message-set variant for subject: {subject}",
        status = 500
    )]
    UnknownSubject {
        /// The delivered subject that no variant routes.
        subject: String,
    },

    /// The server refused a subscription this service declared.
    ///
    /// Not retryable: only an operator can change the authorization configuration.
    #[error(
        code = "NATS_019",
        message = "NATS refused the subscription: {subscription}",
        status = 503,
        retryable = false
    )]
    SubscriptionRefused {
        /// The declared subscription the server refused, and its queue group.
        subscription: String,
    },
    /// A NATS password or token the secret store holds could not be read.
    ///
    /// Names where the secret is, never what it is; retryable only when the store was
    /// unreachable.
    #[error(
        code = "NATS_023",
        message = "The NATS credential at {source_ref} could not be read: {answer}",
        status = 500
    )]
    SecretUnreadable {
        /// Where the secret is: `vault_kv <mount>/<path>#<key>`.
        source_ref: String,
        /// Whether the store could be asked; refines retryability.
        #[overrides]
        answer: StoreAnswer,
        /// What the secret store reported.
        #[source]
        cause: SecretError,
    },

    /// The Transit key named as a NATS NKey's signer cannot be that NKey.
    ///
    /// Not retryable: an operator must fix the key or configuration.
    #[error(
        code = "NATS_024",
        message = "The NKey signer {signer} cannot sign as {public_key}: {reason}",
        status = 500
    )]
    NkeyUnusable {
        /// The signer: `vault_transit <mount>/<key>`.
        signer: String,
        /// The public key it was to sign as.
        public_key: String,
        /// What is wrong with it.
        reason: String,
    },

    /// The NKey signer could not answer: the key could not be read, or a nonce could not be
    /// signed within the deadline.
    ///
    ///
    /// Retryable when the store could not be reached.
    #[error(
        code = "NATS_025",
        message = "The NKey signer {signer} could not answer ({answer}): {reason}",
        status = 500
    )]
    NkeySignerFailed {
        /// The signer: `vault_transit <mount>/<key>`.
        signer: String,
        /// Whether the store could be asked; refines retryability.
        #[overrides]
        answer: StoreAnswer,
        /// What happened.
        reason: String,
    },

    /// A worker's handler needs a value its worker was not given, such as an `Inbox`.
    #[error(
        code = "NATS_026",
        message = "The worker {worker} needs {needs}, which its WorkerContext was not provided",
        status = 500
    )]
    WorkerNeeds {
        /// The handler.
        worker: String,
        /// What it needs, as a reader names it.
        needs: &'static str,
    },
}

impl NatsError {
    /// A connection error from a string message.
    pub fn connection(msg: impl Into<String>) -> Self {
        Self::Connection(msg.into())
    }

    /// A subscription error from a string message.
    pub fn subscription(msg: impl Into<String>) -> Self {
        Self::Subscription(msg.into())
    }

    /// A publish error from a string message.
    pub fn publish(msg: impl Into<String>) -> Self {
        Self::Publish(msg.into())
    }

    /// A request timeout after `timeout_ms` milliseconds.
    #[must_use]
    pub fn timeout(timeout_ms: u64) -> Self {
        Self::Timeout { timeout_ms }
    }

    /// An invalid-subject error naming the subject and the reason.
    pub fn invalid_subject(subject: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidSubject {
            subject: subject.into(),
            reason: reason.into(),
        }
    }

    /// A configuration error from a string message.
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// An internal error from a string message.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// An unknown-subject error naming the subject that no message-set variant routes.
    pub fn unknown_subject(subject: impl Into<String>) -> Self {
        Self::UnknownSubject {
            subject: subject.into(),
        }
    }

    /// Whether this error indicates a connection-level fault.
    #[must_use]
    pub fn is_connection_error(&self) -> bool {
        matches!(
            self,
            Self::Connection(_) | Self::ConnectionClosed { .. } | Self::Authentication(_)
        )
    }
}

impl From<async_nats::ConnectError> for NatsError {
    fn from(err: async_nats::ConnectError) -> Self {
        match err.kind() {
            // Waiting changes neither, so neither is retryable.
            async_nats::ConnectErrorKind::AuthorizationViolation
            | async_nats::ConnectErrorKind::Authentication => Self::Authentication(err.to_string()),
            _ => Self::Connection(err.to_string()),
        }
    }
}

impl From<async_nats::SubscribeError> for NatsError {
    fn from(err: async_nats::SubscribeError) -> Self {
        Self::Subscription(err.to_string())
    }
}

impl From<async_nats::PublishError> for NatsError {
    fn from(err: async_nats::PublishError) -> Self {
        Self::Publish(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    fn codes_and_statuses_are_stable() {
        assert_eq!(NatsError::connection("down").error_code(), "NATS_001");
        assert_eq!(NatsError::connection("down").http_status_code(), 503);
        assert_eq!(NatsError::timeout(30).error_code(), "NATS_006");
        assert_eq!(NatsError::timeout(30).http_status_code(), 504);
        assert_eq!(
            NatsError::invalid_subject("a..b", "consecutive dots").http_status_code(),
            400
        );
        assert_eq!(
            NatsError::MessageTooLarge {
                size: 2_000_000,
                max_size: 1_000_000
            }
            .http_status_code(),
            413
        );
    }

    #[test]
    fn transient_errors_are_retryable() {
        assert!(NatsError::connection("x").is_retryable());
        assert!(NatsError::timeout(1).is_retryable());
        assert!(
            NatsError::NoResponders {
                subject: "s".into()
            }
            .is_retryable()
        );
        assert!(!NatsError::publish("x").is_retryable());
        assert!(!NatsError::config("x").is_retryable());
    }

    #[test]
    fn connection_errors_are_classified() {
        assert!(NatsError::connection("x").is_connection_error());
        assert!(NatsError::Authentication("bad".into()).is_connection_error());
        assert!(!NatsError::publish("x").is_connection_error());
    }

    #[test]
    fn serialization_errors_convert_from_serde() {
        let bad = serde_json::from_slice::<u32>(b"not json").unwrap_err();
        let err: NatsError = bad.into();
        assert_eq!(err.error_code(), "NATS_008");
    }

    #[test]
    fn unknown_subject_has_stable_code_and_is_not_retryable() {
        let err = NatsError::unknown_subject("events.stray");
        assert_eq!(err.error_code(), "NATS_018");
        assert_eq!(err.http_status_code(), 500);
        assert!(!err.is_retryable());
        assert!(err.to_string().contains("events.stray"));
    }

    #[test]
    fn signal_error_has_stable_code_and_is_not_retryable() {
        let err = NatsError::Signal {
            source: std::io::Error::other("no signal handler"),
        };
        assert_eq!(err.error_code(), "NATS_017");
        assert_eq!(err.http_status_code(), 500);
        assert!(!err.is_retryable());
    }
}
