//! [`DeviceAck`] — the server's acknowledgement of a device lifecycle action.

use serde::{Deserialize, Serialize};

/// Acknowledges an enroll, rotate, or revoke request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAck {
    /// Echoes the originating request's `request_id`.
    pub request_id: u64,
    /// Whether the action was applied.
    pub ok: bool,
}

impl DeviceAck {
    /// Builds an ack for an applied action.
    #[must_use]
    pub fn applied(request_id: u64) -> Self {
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
        let ack = DeviceAck::applied(5);
        let back: DeviceAck =
            serde_json::from_str(&serde_json::to_string(&ack).expect("ser")).expect("de");
        assert_eq!(back, ack);
    }
}
