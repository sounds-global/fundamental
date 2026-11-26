//! [`RotateRequest`] — rotate a device to a new public key.

use serde::{Deserialize, Serialize};

use crate::wire::{PROTOCOL_VERSION, VersionedRequest};

/// Rotates a device's key; the previous one still verifies during a grace window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotateRequest {
    /// The wire protocol version the client speaks.
    pub version: u32,
    /// Correlates the matching [`DeviceAck`](crate::wire::DeviceAck).
    pub request_id: u64,
    /// The device being rotated.
    pub device_id: String,
    /// The new public key, as unpadded base64.
    pub new_public_key: String,
}

impl RotateRequest {
    /// Builds a rotation request stamped with the current [`PROTOCOL_VERSION`].
    #[must_use]
    pub fn new(
        request_id: u64,
        device_id: impl Into<String>,
        new_public_key: impl Into<String>,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            device_id: device_id.into(),
            new_public_key: new_public_key.into(),
        }
    }
}

impl VersionedRequest for RotateRequest {
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
        let req = RotateRequest::new(2, "device-1", "bmV3a2V5");
        let back: RotateRequest =
            serde_json::from_str(&serde_json::to_string(&req).expect("ser")).expect("de");
        assert_eq!(back, req);
    }
}
