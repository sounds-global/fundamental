/// A challenge the server issues at the start of a vault connection: a random 32-byte
/// `nonce` the device must sign with its owner-auth key, valid until `expires_at_ms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaultChallenge {
    nonce: [u8; 32],
    expires_at_ms: i64,
}

impl VaultChallenge {
    /// Creates a challenge for `nonce` expiring at `expires_at_ms` (epoch millis).
    #[must_use]
    pub fn new(nonce: [u8; 32], expires_at_ms: i64) -> Self {
        Self {
            nonce,
            expires_at_ms,
        }
    }

    /// The 32-byte nonce the device signs.
    #[must_use]
    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    /// The expiry (epoch millis) after which the challenge is no longer valid.
    #[must_use]
    pub fn expires_at_ms(&self) -> i64 {
        self.expires_at_ms
    }
}
