//! [`VaultChallengeRequest`] — a device asks the server for a vault challenge nonce.

use serde::{Deserialize, Serialize};

use crate::wire::{PROTOCOL_VERSION, VersionedRequest};

/// The first frame of the E2EE-vault handshake: asks for a single-use challenge nonce to sign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultChallengeRequest {
    /// The wire protocol version.
    pub version: u32,
    /// The client-chosen id echoed back on the response.
    pub request_id: u64,
}

impl VaultChallengeRequest {
    /// Builds a challenge request at the current [`PROTOCOL_VERSION`].
    #[must_use]
    pub fn new(request_id: u64) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
        }
    }
}

impl VersionedRequest for VaultChallengeRequest {
    fn protocol_version(&self) -> u32 {
        self.version
    }

    fn request_id(&self) -> u64 {
        self.request_id
    }
}
