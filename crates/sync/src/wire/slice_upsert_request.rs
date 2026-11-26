//! [`SliceUpsertRequest`] — the wire request to write a row back through a slice.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::wire::{PROTOCOL_VERSION, VersionedRequest, WireRecord};

/// A request to push a locally-changed `record` through a slice, answered by an
/// [`UpsertAck`](crate::wire::UpsertAck).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceUpsertRequest {
    /// The wire protocol version the client speaks.
    pub version: u32,
    /// Correlates the matching [`UpsertAck`](crate::wire::UpsertAck).
    pub request_id: u64,
    /// The registered slice name whose base table receives the write.
    pub slice: String,
    /// The slice's parameters.
    #[serde(default)]
    pub params: Value,
    /// The row to write.
    pub record: WireRecord,
    /// The pushing device's id, or empty for an unsigned client.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device_id: String,
    /// The device's signature over `canonical_bytes(slice, record)`, or empty when unsigned.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signature: String,
}

impl SliceUpsertRequest {
    /// Builds an unsigned slice upsert request at the current [`PROTOCOL_VERSION`].
    #[must_use]
    pub fn new(
        request_id: u64,
        slice: impl Into<String>,
        params: Value,
        record: WireRecord,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            slice: slice.into(),
            params,
            record,
            device_id: String::new(),
            signature: String::new(),
        }
    }

    /// Attaches the pushing device's id and signature.
    #[must_use]
    pub fn signed(mut self, device_id: impl Into<String>, signature: impl Into<String>) -> Self {
        self.device_id = device_id.into();
        self.signature = signature.into();
        self
    }
}

impl VersionedRequest for SliceUpsertRequest {
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
    use crate::wire::WireMeta;
    use std::collections::BTreeMap;

    fn wire_record() -> WireRecord {
        WireRecord {
            id: "n1".into(),
            meta: WireMeta {
                updated_at: 5,
                version: 1,
                tombstone: false,
            },
            payload: BTreeMap::new(),
        }
    }

    #[test]
    fn round_trips() {
        let req = SliceUpsertRequest::new(3, "mine", Value::Null, wire_record());
        let back: SliceUpsertRequest =
            serde_json::from_str(&serde_json::to_string(&req).expect("ser")).expect("de");
        assert_eq!(back, req);
        assert_eq!(back.version, PROTOCOL_VERSION);
        assert_eq!(back.slice, "mine");
    }
}
