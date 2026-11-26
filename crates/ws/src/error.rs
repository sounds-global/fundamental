//! [`WsError`], the error type for every fallible WebSocket operation.

use fundamental_macros::Error;

use fundamental_core::error::PayloadRejection;

use fundamental_core::validate::ValidationErrors;

/// Result alias for fallible WebSocket operations.
pub type WsResult<T> = Result<T, WsError>;

/// An error from a WebSocket operation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WsError {
    /// Failed to bind, accept, or upgrade a connection.
    #[error(
        code = "WS_001",
        message = "Connection error: {0}",
        status = 503,
        retryable
    )]
    Connection(String),

    /// An inbound frame, or the payload inside it, did not deserialize; carries the class and position only.
    #[error(code = "WS_002", message = "Invalid message: {0}", status = 400)]
    InvalidMessage(PayloadRejection),

    /// No handler is registered for the inbound action.
    #[error(code = "WS_003", message = "Unknown action", status = 404)]
    UnknownAction,

    /// A payload failed to (de)serialize.
    #[error(code = "WS_004", message = "Serialization error: {0}", status = 500)]
    Serialization(#[from] serde_json::Error),

    /// The connection has closed.
    #[error(
        code = "WS_005",
        message = "Connection closed",
        status = 503,
        retryable
    )]
    Closed,

    /// An unexpected internal error.
    #[error(code = "WS_006", message = "Internal error: {0}", status = 500)]
    Internal(String),

    // `WS_007` is retired, and a code never changes meaning.
    /// A principal was verified but no authenticated slot is free; the connection closes with `1013`.
    #[error(
        code = "WS_008",
        message = "Server at capacity",
        status = 503,
        retryable
    )]
    Saturated,

    /// A principal is already bound to this connection; one connection is one principal.
    #[error(
        code = "WS_009",
        message = "Connection already authenticated",
        status = 409
    )]
    AlreadyAuthenticated,

    /// The server's configuration breaks a rule it must keep to be served.
    #[error(
        code = "WS_010",
        message = "Invalid WebSocket configuration",
        developer_message = "websocket configuration refused: {problems}",
        status = 500
    )]
    InvalidConfig {
        /// Every problem, by field.
        problems: ValidationErrors,
    },

    /// A frame larger than an unauthenticated connection may send; it was refused unparsed.
    #[error(
        code = "WS_011",
        message = "Frame too large before authentication",
        status = 413
    )]
    FrameTooLarge,

    /// A binary frame; the protocol carries JSON text envelopes only.
    #[error(
        code = "WS_012",
        message = "Binary frames are not supported",
        status = 415
    )]
    BinaryUnsupported,

    /// The principal already holds as many connections as one principal may.
    #[error(
        code = "WS_013",
        message = "Too many connections for this principal",
        status = 429
    )]
    PrincipalLimit,

    /// The connection has not been admitted, and may not do this until it is.
    #[error(code = "WS_014", message = "Connection not admitted", status = 403)]
    NotAdmitted,
}

impl WsError {
    /// Creates a [`WsError::Connection`] from any displayable value.
    pub fn connection(message: impl Into<String>) -> Self {
        Self::Connection(message.into())
    }

    /// Creates a [`WsError::InvalidMessage`] from a serde failure, keeping its class and position only.
    pub fn invalid_message(error: &serde_json::Error) -> Self {
        Self::InvalidMessage(PayloadRejection::of(error))
    }

    /// Creates a [`WsError::Internal`] from any displayable value.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    fn codes_and_statuses_are_stable() {
        assert_eq!(WsError::connection("x").error_code(), "WS_001");
        assert_eq!(WsError::connection("x").http_status_code(), 503);
        assert!(WsError::connection("x").is_retryable());

        let not_json = serde_json::from_str::<u8>("x").expect_err("not JSON");
        assert_eq!(WsError::invalid_message(&not_json).error_code(), "WS_002");
        assert_eq!(WsError::invalid_message(&not_json).http_status_code(), 400);
        assert!(!WsError::invalid_message(&not_json).is_retryable());

        let unknown = WsError::UnknownAction;
        assert_eq!(unknown.error_code(), "WS_003");
        assert_eq!(unknown.http_status_code(), 404);

        assert_eq!(WsError::Closed.error_code(), "WS_005");
        assert!(WsError::Closed.is_retryable());
    }

    #[test]
    fn an_invalid_message_quotes_nothing_of_the_frame() {
        let mistyped = serde_json::from_str::<u8>(r#""jan@example.nl""#).expect_err("not a u8");
        let error = WsError::invalid_message(&mistyped);
        assert!(!format!("{error} {error:?}").contains("jan@example.nl"));
    }

    #[test]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn admission_errors_have_stable_codes() {
        assert_eq!(WsError::Saturated.error_code(), "WS_008");
        assert_eq!(WsError::Saturated.http_status_code(), 503);
        assert!(WsError::Saturated.is_retryable());

        assert_eq!(WsError::AlreadyAuthenticated.error_code(), "WS_009");
        assert_eq!(WsError::AlreadyAuthenticated.http_status_code(), 409);
        assert!(!WsError::AlreadyAuthenticated.is_retryable());

        let invalid = WsError::InvalidConfig {
            problems: ValidationErrors::new(),
        };
        assert_eq!(invalid.error_code(), "WS_010");
        assert_eq!(invalid.http_status_code(), 500);

        assert_eq!(WsError::FrameTooLarge.error_code(), "WS_011");
        assert_eq!(WsError::FrameTooLarge.http_status_code(), 413);
        assert!(!WsError::FrameTooLarge.is_retryable());

        assert_eq!(WsError::BinaryUnsupported.error_code(), "WS_012");
        assert_eq!(WsError::BinaryUnsupported.http_status_code(), 415);

        assert_eq!(WsError::PrincipalLimit.error_code(), "WS_013");
        assert_eq!(WsError::PrincipalLimit.http_status_code(), 429);
        assert!(!WsError::PrincipalLimit.is_retryable());

        assert_eq!(WsError::NotAdmitted.error_code(), "WS_014");
        assert_eq!(WsError::NotAdmitted.http_status_code(), 403);
    }
}
