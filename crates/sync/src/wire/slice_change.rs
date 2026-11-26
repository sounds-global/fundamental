//! [`SliceChange`] — one live change pushed to a subscribed slice.

use serde::{Deserialize, Serialize};

use crate::wire::WireRecord;

/// The payload of an unsolicited `slice.change` frame: one changed row, routed by `table`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceChange {
    /// The base table the changed row belongs to.
    pub table: String,
    /// The changed row (a tombstone marks a delete).
    pub record: WireRecord,
}

impl SliceChange {
    /// Creates a change frame for a row in `table`.
    #[must_use]
    pub fn new(table: impl Into<String>, record: WireRecord) -> Self {
        Self {
            table: table.into(),
            record,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::WireMeta;
    use std::collections::BTreeMap;

    fn wire_record(id: &str) -> WireRecord {
        WireRecord {
            id: id.into(),
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
        let change = SliceChange::new("notes", wire_record("n1"));
        let back: SliceChange =
            serde_json::from_str(&serde_json::to_string(&change).expect("ser")).expect("de");
        assert_eq!(back, change);
        assert_eq!(back.table, "notes");
        assert_eq!(back.record.id, "n1");
    }
}
