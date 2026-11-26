//! [`ErrorReply`] — a correlated error response for one request.

use serde::{Deserialize, Serialize};

/// An error response correlated to a request by `request_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorReply {
    /// The request this error answers.
    pub request_id: u64,
    /// The error detail.
    pub error: WireError,
}

/// The code and user-safe message of a [`ErrorReply`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    /// The stable error code (for example `AUTH_001`).
    pub code: String,
    /// A user-safe message.
    pub message: String,
}

impl ErrorReply {
    /// Builds an error reply for `request_id` from a code and message.
    #[must_use]
    pub fn new(request_id: u64, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            request_id,
            error: WireError {
                code: code.into(),
                message: message.into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_in_the_shape_the_client_correlates_on() {
        let reply = ErrorReply::new(9, "AUTH_001", "no token");
        let value = serde_json::to_value(&reply).expect("serialize");
        assert_eq!(value["request_id"], 9);
        assert_eq!(value["error"]["code"], "AUTH_001");
        assert_eq!(value["error"]["message"], "no token");
    }
}
