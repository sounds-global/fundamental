//! A single captured row change, ready to fan out to live subscribers.

use crate::engine::SyncRecord;

/// A row change captured from a server backend, carrying everything a subscriber needs to fan it
/// out without reading the database again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncChange {
    /// The principal the changed row belongs to, by which it is routed.
    pub owner: String,
    /// The base table the change occurred in.
    pub table: String,
    /// The row's new state: a self-contained snapshot, or a tombstone for a delete.
    pub record: SyncRecord,
    /// The change's commit timestamp in milliseconds since the Unix epoch.
    pub commit_ts: i64,
}

impl SyncChange {
    /// Creates a change for `owner`'s row in `table`, captured at `commit_ts`.
    pub fn new(
        owner: impl Into<String>,
        table: impl Into<String>,
        record: SyncRecord,
        commit_ts: i64,
    ) -> Self {
        Self {
            owner: owner.into(),
            table: table.into(),
            record,
            commit_ts,
        }
    }

    /// Whether this change deletes the row (its record is a tombstone).
    #[must_use]
    pub fn is_delete(&self) -> bool {
        self.record.meta.tombstone
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::SyncMeta;
    use std::collections::BTreeMap;

    fn record(id: &str, tombstone: bool) -> SyncRecord {
        let meta = SyncMeta::new(10, 1);
        let meta = if tombstone { meta.tombstoned() } else { meta };
        SyncRecord::new(id, meta, BTreeMap::new())
    }

    #[test]
    fn new_carries_owner_table_and_commit_ts() {
        let change = SyncChange::new("alice", "notes", record("n1", false), 42);
        assert_eq!(change.owner, "alice");
        assert_eq!(change.table, "notes");
        assert_eq!(change.record.id, "n1");
        assert_eq!(change.commit_ts, 42);
        assert!(!change.is_delete());
    }

    #[test]
    fn a_tombstone_record_is_a_delete() {
        let change = SyncChange::new("alice", "notes", record("n1", true), 99);
        assert!(change.is_delete());
    }
}
