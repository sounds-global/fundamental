//! [`UpsertAck`] — the wire response for a slice write-back.

use serde::{Deserialize, Serialize};

/// The server's acknowledgement that a
/// [`SliceUpsertRequest`](crate::wire::SliceUpsertRequest) was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpsertAck {
    /// Echoes the originating request's `request_id`.
    pub request_id: u64,
    /// Whether the write was applied.
    pub ok: bool,
}

impl UpsertAck {
    /// Builds an ack for an applied write.
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
    fn applied_round_trips() {
        let ack = UpsertAck::applied(7);
        let back: UpsertAck =
            serde_json::from_str(&serde_json::to_string(&ack).expect("ser")).expect("de");
        assert_eq!(back.request_id, 7);
        assert!(back.ok);
    }
}
