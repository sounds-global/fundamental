//! [`WireRecord`] — the wire form of a synced row.

use std::collections::BTreeMap;

use fundamental_db::RecordKey;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::wire::WireMeta;
use crate::{SyncError, SyncMeta, SyncRecord};

/// The on-the-wire form of a [`SyncRecord`]; inbound conversion validates the untrusted metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireRecord {
    /// The row's key columns, in key order: a bare string for one column, an array for several.
    pub id: RecordKey,
    /// Conflict-resolution metadata.
    pub meta: WireMeta,
    /// The synced data columns: name to rendered value.
    #[serde(default)]
    pub payload: BTreeMap<String, String>,
}

impl From<&SyncRecord> for WireRecord {
    fn from(record: &SyncRecord) -> Self {
        Self {
            id: record.id.clone(),
            meta: WireMeta::from(record.meta),
            payload: record.payload.clone(),
        }
    }
}

impl TryFrom<WireRecord> for SyncRecord {
    type Error = SyncError;

    /// Validates the inbound metadata and builds a [`SyncRecord`].
    ///
    /// # Errors
    /// Returns [`SyncError::Protocol`] if `updated_at` or `version` is negative, naming only the field.
    #[instrument(name = "sync.wire.record", skip_all, err(level = "warn"))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn try_from(wire: WireRecord) -> Result<Self, Self::Error> {
        if wire.meta.updated_at < 0 {
            return Err(SyncError::Protocol(
                "a record's updated_at must be non-negative".to_owned(),
            ));
        }
        if wire.meta.version < 0 {
            return Err(SyncError::Protocol(
                "a record's version must be non-negative".to_owned(),
            ));
        }
        let mut meta = SyncMeta::new(wire.meta.updated_at, wire.meta.version);
        if wire.meta.tombstone {
            meta = meta.tombstoned();
        }
        Ok(SyncRecord::new(wire.id, meta, wire.payload))
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    fn record(meta: SyncMeta) -> SyncRecord {
        let mut payload = BTreeMap::new();
        payload.insert("title".to_string(), "hello".to_string());
        SyncRecord::new("row-1", meta, payload)
    }

    #[test]
    fn round_trips_a_live_record() {
        let original = record(SyncMeta::new(100, 3));
        let wire = WireRecord::from(&original);
        let back = SyncRecord::try_from(wire).expect("valid");
        assert_eq!(back, original);
    }

    #[test]
    fn round_trips_a_tombstone() {
        let original = record(SyncMeta::new(200, 4).tombstoned());
        let wire = WireRecord::from(&original);
        let back = SyncRecord::try_from(wire).expect("valid");
        assert!(back.meta.tombstone);
        assert_eq!(back, original);
    }

    #[test]
    fn serializes_with_stable_keys() {
        let wire = WireRecord::from(&record(SyncMeta::new(1, 1)));
        let value = serde_json::to_value(&wire).expect("serialize");
        assert_eq!(value["id"], "row-1");
        assert_eq!(value["meta"]["updated_at"], 1);
        assert_eq!(value["payload"]["title"], "hello");
    }

    #[test]
    fn payload_defaults_to_empty_when_absent() {
        let wire: WireRecord = serde_json::from_str(
            r#"{"id":"x","meta":{"updated_at":1,"version":1,"tombstone":false}}"#,
        )
        .expect("valid");
        assert!(wire.payload.is_empty());
    }

    #[test]
    fn rejects_negative_updated_at_with_protocol_error() {
        let wire = WireRecord {
            id: "jan@example.nl".into(),
            meta: WireMeta {
                updated_at: -31_612_345_678,
                version: 0,
                tombstone: false,
            },
            payload: BTreeMap::new(),
        };
        let err = SyncRecord::try_from(wire).expect_err("must reject");
        assert!(matches!(err, SyncError::Protocol(_)));
        assert_eq!(err.error_code(), "SYNC_005");
        assert_eq!(err.http_status_code(), 422);
        let shown = format!("{err} {err:?}");
        assert!(shown.contains("updated_at"), "names the field: {shown}");
        assert!(
            !shown.contains("jan@example.nl") && !shown.contains("31612345678"),
            "quotes neither the client's id nor its value: {shown}"
        );
    }

    #[test]
    fn rejects_negative_version_with_protocol_error() {
        let wire = WireRecord {
            id: "bad-version".into(),
            meta: WireMeta {
                updated_at: 0,
                version: -5,
                tombstone: false,
            },
            payload: BTreeMap::new(),
        };
        let err = SyncRecord::try_from(wire).expect_err("must reject");
        assert!(matches!(err, SyncError::Protocol(_)));
        assert!(err.to_string().contains("version"));
    }
}
