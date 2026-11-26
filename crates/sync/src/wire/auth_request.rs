//! [`AuthRequest`] — the first frame a device sends to authenticate a connection.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::wire::{PROTOCOL_VERSION, VersionedRequest};

/// Authenticates the connection by presenting a bearer token in its first frame.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthRequest {
    /// The wire protocol version the client speaks.
    pub version: u32,
    /// Correlates the matching [`AuthAck`](crate::wire::AuthAck).
    pub request_id: u64,
    /// The bearer token to verify.
    pub token: String,
}

impl AuthRequest {
    /// Builds an auth request stamped with the current [`PROTOCOL_VERSION`].
    #[must_use]
    pub fn new(request_id: u64, token: impl Into<String>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            token: token.into(),
        }
    }
}

/// Never prints the token.
impl fmt::Debug for AuthRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthRequest")
            .field("version", &self.version)
            .field("request_id", &self.request_id)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

impl VersionedRequest for AuthRequest {
    fn protocol_version(&self) -> u32 {
        self.version
    }

    fn request_id(&self) -> u64 {
        self.request_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_stamps_version() {
        let req = AuthRequest::new(1, "jwt-token");
        let back: AuthRequest =
            serde_json::from_str(&serde_json::to_string(&req).expect("ser")).expect("de");
        assert_eq!(back.version, PROTOCOL_VERSION);
        assert_eq!(back.token, "jwt-token");
        assert_eq!(back.request_id, 1);
    }

    #[test]
    fn a_request_never_prints_its_token() {
        let req = AuthRequest::new(1, "eyJhbGciOiJIUzI1NiJ9.secret.sig");
        let shown = format!("{req:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("[REDACTED]"));
    }
}
