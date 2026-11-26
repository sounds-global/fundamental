//! [`SliceSubscribeAck`] — the server's confirmation of a slice subscription.

use serde::{Deserialize, Serialize};

/// Acknowledges a [`SliceSubscribeRequest`](crate::wire::SliceSubscribeRequest): live changes
/// will now be pushed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceSubscribeAck {
    /// Correlates the originating subscribe request.
    pub request_id: u64,
    /// The slice now subscribed to.
    pub slice: String,
    /// Always `true`; present so the frame is unambiguously a subscribe ack.
    pub subscribed: bool,
}

impl SliceSubscribeAck {
    /// Confirms the subscription to `slice` for `request_id`.
    #[must_use]
    pub fn new(request_id: u64, slice: impl Into<String>) -> Self {
        Self {
            request_id,
            slice: slice.into(),
            subscribed: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_is_marked_subscribed() {
        let ack = SliceSubscribeAck::new(9, "mine");
        let back: SliceSubscribeAck =
            serde_json::from_str(&serde_json::to_string(&ack).expect("ser")).expect("de");
        assert_eq!(back, ack);
        assert!(back.subscribed);
        assert_eq!(back.request_id, 9);
    }
}
