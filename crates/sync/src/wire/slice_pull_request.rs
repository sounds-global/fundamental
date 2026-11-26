//! [`SlicePullRequest`] — the wire request to pull a slice's changed rows.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::wire::{PROTOCOL_VERSION, SliceCursor, VersionedRequest};

/// A request for the rows of a named slice changed since a high-water mark, paged by `cursor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlicePullRequest {
    /// The wire protocol version the client speaks.
    pub version: u32,
    /// Correlates the matching [`SlicePullResponse`](crate::wire::SlicePullResponse).
    pub request_id: u64,
    /// The registered slice name to pull from.
    pub slice: String,
    /// The slice's parameters.
    #[serde(default)]
    pub params: Value,
    /// The high-water mark: rows with a greater `updated_at` are returned.
    pub since: i64,
    /// The keyset continuation point for the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<SliceCursor>,
    /// The maximum number of records in one page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl SlicePullRequest {
    /// Builds a first-page slice pull request at the current [`PROTOCOL_VERSION`].
    #[must_use]
    pub fn new(request_id: u64, slice: impl Into<String>, params: Value, since: i64) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            slice: slice.into(),
            params,
            since,
            cursor: None,
            limit: None,
        }
    }

    /// Sets the continuation cursor for the next page.
    #[must_use]
    pub fn with_cursor(mut self, cursor: SliceCursor) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// Sets the maximum page size.
    #[must_use]
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}

impl VersionedRequest for SlicePullRequest {
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
        let req = SlicePullRequest::new(7, "favorites", json!({ "list": "a" }), 99);
        assert_eq!(req.version, PROTOCOL_VERSION);
        let value = serde_json::to_value(&req).expect("serialize");
        assert_eq!(value["request_id"], 7);
        assert_eq!(value["slice"], "favorites");
        assert_eq!(value["params"]["list"], "a");
        assert_eq!(value["since"], 99);
        assert!(value.get("cursor").is_none());
    }

    #[test]
    fn round_trips_with_default_params() {
        let req = SlicePullRequest::new(1, "mine", Value::Null, 0).with_limit(50);
        let back: SlicePullRequest =
            serde_json::from_str(&serde_json::to_string(&req).expect("ser")).expect("de");
        assert_eq!(back, req);
        assert_eq!(back.limit, Some(50));
    }
}
