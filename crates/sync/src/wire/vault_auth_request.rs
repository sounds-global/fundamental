//! [`VaultAuthRequest`] — a device proves vault ownership by signing the challenge.

use serde::{Deserialize, Serialize};

use crate::wire::{PROTOCOL_VERSION, VersionedRequest};

/// The second frame of the E2EE-vault handshake: the owner public key and its signature over the
/// challenge `nonce`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultAuthRequest {
    /// The wire protocol version.
    pub version: u32,
    /// Echoes the challenge's request id.
    pub request_id: u64,
    /// The owner-auth public key (unpadded base64), whose commitment is the owner id.
    pub owner_public_key: String,
    /// The challenge nonce the server issued.
    pub nonce: Vec<u8>,
    /// The Ed25519 signature over the domain-separated nonce (unpadded base64).
    pub signature: String,
}

impl VaultAuthRequest {
    /// Builds a vault auth request at the current [`PROTOCOL_VERSION`].
    #[must_use]
    pub fn new(
        request_id: u64,
        owner_public_key: impl Into<String>,
        nonce: Vec<u8>,
        signature: impl Into<String>,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            owner_public_key: owner_public_key.into(),
            nonce,
            signature: signature.into(),
        }
    }
}

impl VersionedRequest for VaultAuthRequest {
    fn protocol_version(&self) -> u32 {
        self.version
    }

    fn request_id(&self) -> u64 {
        self.request_id
    }
}
