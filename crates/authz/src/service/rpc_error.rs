//! [`RpcError`]: the wire projection of an [`AuthzError`] carried in an RPC reply.

use fundamental_core::Error;
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::error::AuthzError;

/// An authorization failure as it travels back to an RPC caller.
///
/// The framework [`Error`](fundamental_core::Error) contract on the wire, so a backend failure
/// is never a silent deny.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    /// The stable machine-readable code (e.g. `AUTHZ_011`).
    pub code: String,
    /// A user-safe description of what failed.
    pub message: String,
    /// The HTTP status the same failure would carry over an HTTP surface.
    pub status: u16,
    /// Whether the caller may retry the request.
    pub retryable: bool,
}

impl RpcError {
    /// Projects an [`AuthzError`] onto the wire.
    #[must_use]
    pub fn from_authz(error: &AuthzError) -> Self {
        Self {
            code: error.error_code().to_string(),
            message: error.display_message().into_owned(),
            status: error.http_status_code(),
            retryable: error.is_retryable(),
        }
    }

    /// Logs `error` once at the service boundary, naming the RPC `operation`, and projects it.
    #[must_use]
    pub fn boundary(operation: &'static str, error: &AuthzError) -> Self {
        error!(
            operation,
            error = %error,
            code = error.error_code(),
            "authorization RPC failed"
        );
        Self::from_authz(error)
    }
}

impl From<&AuthzError> for RpcError {
    fn from(error: &AuthzError) -> Self {
        Self::from_authz(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_the_error_contract_onto_the_wire() {
        let error = AuthzError::DepthExceeded { depth: 32 };
        let wire = RpcError::from_authz(&error);
        assert_eq!(wire.code, "AUTHZ_010");
        assert_eq!(wire.status, error.http_status_code());
        assert_eq!(wire.retryable, error.is_retryable());
        assert!(!wire.message.is_empty());
    }

    #[test]
    fn round_trips_through_json() {
        let wire = RpcError::from_authz(&AuthzError::BudgetExceeded { budget: 1000 });
        let bytes = serde_json::to_vec(&wire).expect("serialize");
        let back: RpcError = serde_json::from_slice(&bytes).expect("deserialize");
        assert_eq!(wire, back);
    }
}
