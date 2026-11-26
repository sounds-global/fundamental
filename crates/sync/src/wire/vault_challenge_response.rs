//! [`VaultChallengeResponse`] — the server's issued challenge nonce.

use serde::{Deserialize, Serialize};

/// The server's reply to a [`VaultChallengeRequest`](crate::wire::VaultChallengeRequest): a
/// single-use `nonce` to sign, valid until `expires_at_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultChallengeResponse {
    /// Echoes the request's id.
    pub request_id: u64,
    /// The 32-byte single-use challenge nonce.
    pub nonce: Vec<u8>,
    /// When the challenge expires (epoch milliseconds).
    pub expires_at_ms: i64,
}

impl VaultChallengeResponse {
    /// Builds a challenge response.
    #[must_use]
    pub fn new(request_id: u64, nonce: Vec<u8>, expires_at_ms: i64) -> Self {
        Self {
            request_id,
            nonce,
            expires_at_ms,
        }
    }
}
