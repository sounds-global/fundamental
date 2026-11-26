//! An in-memory [`SliceStore`] for tests and local development.

use std::collections::HashMap;
use std::sync::Mutex;

use fundamental_db::{Filter, RecordKey};

use crate::slice_match::record_matches;
use crate::wire::SliceCursor;
use crate::{SliceStore, SliceWriteStore, SyncError, SyncRecord};

/// An in-memory [`SliceStore`] that evaluates slice [`Filter`]s against records held in memory.
#[derive(Debug, Default)]
pub struct MemorySliceStore {
    tables: Mutex<HashMap<String, HashMap<RecordKey, SyncRecord>>>,
}

impl MemorySliceStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces `record` in `table` by its key.
    ///
    /// # Panics
    /// Panics if the internal mutex is poisoned.
    pub fn put(&self, table: &str, record: SyncRecord) {
        self.tables
            .lock()
            .expect("slice store mutex poisoned")
            .entry(table.to_string())
            .or_default()
            .insert(record.id.clone(), record);
    }
}

impl SliceStore for MemorySliceStore {
    async fn changed_page(
        &self,
        table: &str,
        _data_columns: &[&str],
        key_columns: &[&str],
        filters: &[Filter],
        since: i64,
        after: Option<&SliceCursor>,
        limit: u32,
    ) -> Result<Vec<SyncRecord>, SyncError> {
        let tables = self
            .tables
            .lock()
            .map_err(|_| SyncError::Storage("slice store mutex poisoned".to_string()))?;

        let mut matched: Vec<SyncRecord> = tables
            .get(table)
            .into_iter()
            .flat_map(HashMap::values)
            .filter(|record| {
                after_keyset(record, since, after)
                    && filters
                        .iter()
                        .all(|filter| record_matches(filter, record, key_columns))
            })
            .cloned()
            .collect();

        matched.sort_by(|a, b| {
            a.meta
                .updated_at
                .cmp(&b.meta.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        matched.truncate(limit as usize);
        Ok(matched)
    }
}

/// Whether `record` falls strictly after the keyset point `after`, or else after `since`.
fn after_keyset(record: &SyncRecord, since: i64, after: Option<&SliceCursor>) -> bool {
    match after {
        None => record.meta.updated_at > since,
        Some(cursor) => {
            record.meta.updated_at > cursor.updated_at
                || (record.meta.updated_at == cursor.updated_at && record.id > cursor.key)
        }
    }
}

impl SliceWriteStore for MemorySliceStore {
    async fn upsert(
        &self,
        table: &str,
        _data_columns: &[&str],
        _key_columns: &[&str],
        record: &SyncRecord,
    ) -> Result<(), SyncError> {
        self.put(table, record.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SyncMeta;
    use std::collections::BTreeMap;

    fn record(id: &str, owner: &str, updated_at: i64) -> SyncRecord {
        let mut payload = BTreeMap::new();
        payload.insert("owner_id".to_string(), owner.to_string());
        SyncRecord {
            id: id.into(),
            meta: SyncMeta::new(updated_at, 1),
            payload,
        }
    }

    #[tokio::test]
    async fn changed_since_filters_scopes_and_advances() {
        let store = MemorySliceStore::new();
        store.put("animals", record("a", "alice", 1000));
        store.put("animals", record("b", "alice", 3000));
        store.put("animals", record("c", "bob", 2000));

        let mine = [Filter::eq("owner_id", "alice")];

        let all = store
            .changed_since("animals", &[], &["id"], &mine, 0)
            .await
            .expect("changed_since");
        let ids: Vec<String> = all.iter().map(|r| r.id.to_string()).collect();
        assert_eq!(ids, vec!["a", "b"]);

        let after = store
            .changed_since("animals", &[], &["id"], &mine, 1000)
            .await
            .expect("changed_since");
        let ids: Vec<String> = after.iter().map(|r| r.id.to_string()).collect();
        assert_eq!(ids, vec!["b"]);
    }

    #[tokio::test]
    async fn keyset_paging_does_not_skip_rows_sharing_an_updated_at() {
        let store = MemorySliceStore::new();
        // Three rows at the same `updated_at`: only a keyset cursor neither skips nor loops.
        store.put("t", record("a", "alice", 1000));
        store.put("t", record("b", "alice", 1000));
        store.put("t", record("c", "alice", 1000));
        let mine = [Filter::eq("owner_id", "alice")];

        let page1 = store
            .changed_page("t", &[], &["id"], &mine, 0, None, 2)
            .await
            .expect("page 1");
        let ids: Vec<String> = page1.iter().map(|r| r.id.to_string()).collect();
        assert_eq!(ids, vec!["a", "b"]);

        let cursor = SliceCursor::new(page1[1].meta.updated_at, page1[1].id.clone());
        let page2 = store
            .changed_page("t", &[], &["id"], &mine, 0, Some(&cursor), 2)
            .await
            .expect("page 2");
        let ids: Vec<String> = page2.iter().map(|r| r.id.to_string()).collect();
        assert_eq!(
            ids,
            vec!["c"],
            "the tied-timestamp row after the cursor, no skip/dup"
        );
    }

    #[tokio::test]
    async fn put_replaces_by_primary_key() {
        let store = MemorySliceStore::new();
        store.put("t", record("a", "alice", 1000));
        store.put("t", record("a", "alice", 5000));
        let rows = store
            .changed_since("t", &[], &["id"], &[], 0)
            .await
            .expect("changed_since");
        assert_eq!(rows.len(), 1, "same id replaces, not appends");
        assert_eq!(rows[0].meta.updated_at, 5000);
    }
}
