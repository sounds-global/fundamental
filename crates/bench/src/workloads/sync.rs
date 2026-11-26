//! Sync's wire: a pulled page, a pushed row, the bytes a device signs, and their envelope.

use std::collections::BTreeMap;

use fundamental_sync::wire::{SlicePullResponse, SliceUpsertRequest, WireRecord, canonical_bytes};
use fundamental_sync::{SyncMeta, SyncRecord};
use fundamental_ws_wire::WsEnvelope;
use serde_json::{Value, json};

/// How many rows a pulled page holds.
pub const ROWS: usize = 100;

/// A page of [`ROWS`] rows, one pushed row, and each one's JSON.
#[derive(Debug, Clone)]
pub struct SyncWire {
    page: SlicePullResponse,
    page_json: Vec<u8>,
    push: SliceUpsertRequest,
    push_json: Vec<u8>,
    record: SyncRecord,
    envelope_json: Vec<u8>,
}

impl SyncWire {
    /// The page, the push and their encodings.
    #[must_use]
    pub fn new() -> Self {
        let records: Vec<SyncRecord> = (0..ROWS).map(record).collect();
        let page = SlicePullResponse::last(7, records.iter().map(WireRecord::from).collect());
        let record = record(ROWS);
        let push = SliceUpsertRequest::new(8, "notes", Value::Null, WireRecord::from(&record));
        let page_json = serde_json::to_vec(&page).unwrap_or_default();
        let push_json = serde_json::to_vec(&push).unwrap_or_default();
        let envelope = WsEnvelope::new(
            "slice.upsert",
            serde_json::to_value(&push).unwrap_or(Value::Null),
        );
        let envelope_json = serde_json::to_vec(&envelope).unwrap_or_default();
        Self {
            page,
            page_json,
            push,
            push_json,
            record,
            envelope_json,
        }
    }

    /// The page encoded as JSON; its length.
    #[must_use]
    pub fn encode_pull(&self) -> usize {
        serde_json::to_vec(&self.page).map_or(0, |bytes| bytes.len())
    }

    /// The page decoded from JSON; how many rows.
    #[must_use]
    pub fn decode_pull(&self) -> usize {
        serde_json::from_slice::<SlicePullResponse>(&self.page_json)
            .map_or(0, |page| page.records.len())
    }

    /// The push encoded and decoded again; whether it came back the same.
    #[must_use]
    pub fn push_round_trip(&self) -> bool {
        serde_json::to_vec(&self.push).is_ok_and(|bytes| {
            serde_json::from_slice::<SliceUpsertRequest>(&bytes)
                .is_ok_and(|back| back.record == self.push.record)
        }) && !self.push_json.is_empty()
    }

    /// The bytes a device signs for the push; their length.
    #[must_use]
    pub fn canonical(&self) -> usize {
        canonical_bytes("notes", &self.record).len()
    }

    /// The envelope read from its frame and its push read out of it; whether it was one.
    #[must_use]
    pub fn envelope_round_trip(&self) -> bool {
        serde_json::from_slice::<WsEnvelope>(&self.envelope_json).is_ok_and(|envelope| {
            envelope.action == "slice.upsert"
                && serde_json::from_value::<SliceUpsertRequest>(envelope.data).is_ok()
        })
    }
}

impl Default for SyncWire {
    fn default() -> Self {
        Self::new()
    }
}

fn record(n: usize) -> SyncRecord {
    let mut payload = BTreeMap::new();
    payload.insert(
        "body".to_owned(),
        format!("A note about project {n}, written in full."),
    );
    payload.insert(
        "owner_id".to_owned(),
        "f1b6c8a0-5d2e-4c1a-9b7e-3a2d1c0b9e8f".to_owned(),
    );
    payload.insert("kind".to_owned(), "note".to_owned());
    payload.insert(
        "meta".to_owned(),
        json!({ "pinned": n.is_multiple_of(2) }).to_string(),
    );
    let at = i64::try_from(n).unwrap_or(i64::MAX);
    SyncRecord::new(
        format!("note-{n:04}").as_str(),
        SyncMeta::new(1_700_000_000_000 + at, 1),
        payload,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wire_form_round_trips() {
        let wire = SyncWire::new();
        assert!(wire.encode_pull() > 0);
        assert_eq!(wire.decode_pull(), ROWS);
        assert!(wire.push_round_trip());
        assert!(wire.canonical() > 0);
        assert!(wire.envelope_round_trip());
    }
}
