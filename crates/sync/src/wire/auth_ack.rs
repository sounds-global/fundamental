//! [`AuthAck`] — the server's confirmation that a connection is authenticated.

use serde::{Deserialize, Serialize};

/// Acknowledges that an [`AuthRequest`](crate::wire::AuthRequest) succeeded and the
/// connection is now authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthAck {
    /// Echoes the originating request's `request_id`.
    pub request_id: u64,
    /// Whether the connection is now authenticated.
    pub ok: bool,
}

impl AuthAck {
    /// Builds an ack for a successful authentication.
    #[must_use]
    pub fn authenticated(request_id: u64) -> Self {
        Self {
            request_id,
            ok: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let ack = AuthAck::authenticated(4);
        let back: AuthAck =
            serde_json::from_str(&serde_json::to_string(&ack).expect("ser")).expect("de");
        assert_eq!(back.request_id, 4);
        assert!(back.ok);
    }
}
