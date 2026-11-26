//! [`RevokeRequest`] — revoke a device's key immediately.

use serde::{Deserialize, Serialize};

use crate::wire::{PROTOCOL_VERSION, VersionedRequest};

/// Revokes a device immediately, with no grace window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevokeRequest {
    /// The wire protocol version the client speaks.
    pub version: u32,
    /// Correlates the matching [`DeviceAck`](crate::wire::DeviceAck).
    pub request_id: u64,
    /// The device being revoked.
    pub device_id: String,
}

impl RevokeRequest {
    /// Builds a revocation request stamped with the current [`PROTOCOL_VERSION`].
    #[must_use]
    pub fn new(request_id: u64, device_id: impl Into<String>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            device_id: device_id.into(),
        }
    }
}

impl VersionedRequest for RevokeRequest {
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
    fn round_trips() {
        let req = RevokeRequest::new(3, "device-1");
        let back: RevokeRequest =
            serde_json::from_str(&serde_json::to_string(&req).expect("ser")).expect("de");
        assert_eq!(back, req);
    }
}
