//! [`SliceSubscribeRequest`] — the wire request to subscribe to a slice's changes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::wire::{PROTOCOL_VERSION, VersionedRequest};

/// A request to receive live [`SliceChange`](crate::wire::SliceChange) frames for a slice; it
/// replays no history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceSubscribeRequest {
    /// The wire protocol version the client speaks.
    pub version: u32,
    /// Correlates the matching subscribe acknowledgement.
    pub request_id: u64,
    /// The registered slice name to subscribe to.
    pub slice: String,
    /// The slice's parameters.
    #[serde(default)]
    pub params: Value,
}

impl SliceSubscribeRequest {
    /// Builds a subscribe request, stamped with the current [`PROTOCOL_VERSION`].
    #[must_use]
    pub fn new(request_id: u64, slice: impl Into<String>, params: Value) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            slice: slice.into(),
            params,
        }
    }
}

impl VersionedRequest for SliceSubscribeRequest {
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
    use serde_json::json;

    #[test]
    fn new_stamps_version_and_carries_slice_and_params() {
        let req = SliceSubscribeRequest::new(7, "favorites", json!({ "list": "a" }));
        assert_eq!(req.version, PROTOCOL_VERSION);
        assert_eq!(req.request_id, 7);
        assert_eq!(req.slice, "favorites");
        assert_eq!(req.params["list"], "a");
    }

    #[test]
    fn round_trips_with_default_params() {
        let req = SliceSubscribeRequest::new(1, "mine", Value::Null);
        let back: SliceSubscribeRequest =
            serde_json::from_str(&serde_json::to_string(&req).expect("ser")).expect("de");
        assert_eq!(back, req);
    }
}
