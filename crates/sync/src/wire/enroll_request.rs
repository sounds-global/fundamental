//! [`EnrollRequest`] — enroll a device's public key with the server.

use serde::{Deserialize, Serialize};

use crate::wire::{PROTOCOL_VERSION, VersionedRequest};

/// Enrolls a device's ed25519 public key, binding it to the authenticated user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollRequest {
    /// The wire protocol version the client speaks.
    pub version: u32,
    /// Correlates the matching [`DeviceAck`](crate::wire::DeviceAck).
    pub request_id: u64,
    /// The device being enrolled.
    pub device_id: String,
    /// The device's public key, as unpadded base64.
    pub public_key: String,
}

impl EnrollRequest {
    /// Builds an enrollment request stamped with the current [`PROTOCOL_VERSION`].
    #[must_use]
    pub fn new(
        request_id: u64,
        device_id: impl Into<String>,
        public_key: impl Into<String>,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            device_id: device_id.into(),
            public_key: public_key.into(),
        }
    }
}

impl VersionedRequest for EnrollRequest {
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
        let req = EnrollRequest::new(1, "device-1", "cHVibGlja2V5");
        let back: EnrollRequest =
            serde_json::from_str(&serde_json::to_string(&req).expect("ser")).expect("de");
        assert_eq!(back, req);
        assert_eq!(back.version, PROTOCOL_VERSION);
    }
}
