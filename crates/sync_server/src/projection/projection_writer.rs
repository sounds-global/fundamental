use std::collections::BTreeMap;
use std::sync::Arc;

use fundamental_sync::{ServerSyncable, SliceWriteStore, SyncError, SyncMeta, SyncRecord};
use tracing::{debug, instrument};

/// Writes recomputed projection rows through the sync overlay, so change capture fans them out.
pub struct ProjectionWriter<S> {
    store: Arc<S>,
}

impl<S: SliceWriteStore + Send + Sync + 'static> ProjectionWriter<S> {
    /// Wraps the backend write store the projection rows are persisted to.
    #[must_use]
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    /// Upserts a recomputed projection row of model `P`, server-stamping the overlay.
    ///
    /// The payload must carry `P`'s owner column, so the change stays scoped.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the store write fails.
    #[instrument(
        name = "sync.projection.write",
        skip_all,
        fields(table = P::TABLE, tombstone = record.meta.tombstone),
        err(Debug)
    )]
    pub async fn write<P: ServerSyncable>(&self, record: &SyncRecord) -> Result<(), SyncError> {
        self.write_record(P::TABLE, P::data_columns(), P::KEY_COLUMNS, record)
            .await
    }

    /// Tombstones the projection row `id` of model `P`, its owner column set to `owner`.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the store write fails.
    #[instrument(
        name = "sync.projection.tombstone",
        skip_all,
        fields(table = P::TABLE, tombstone = true),
        err(Debug)
    )]
    pub async fn tombstone<P: ServerSyncable>(
        &self,
        id: &str,
        owner: &str,
    ) -> Result<(), SyncError> {
        let record = tombstone_record(P::OWNER_COLUMN, id, owner);
        self.write_record(P::TABLE, P::data_columns(), P::KEY_COLUMNS, &record)
            .await
    }

    /// The non-generic write path the typed methods and the refresh loop share.
    pub(crate) async fn write_record(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        record: &SyncRecord,
    ) -> Result<(), SyncError> {
        self.store
            .upsert(table, data_columns, key_columns, record)
            .await?;
        debug!(table, "projection row rewritten");
        Ok(())
    }
}

/// A tombstone record carrying only the owner column; the store stamps `updated_at` and
/// `version`.
pub(crate) fn tombstone_record(owner_column: Option<&str>, id: &str, owner: &str) -> SyncRecord {
    let mut payload = BTreeMap::new();
    if let Some(owner_column) = owner_column {
        payload.insert(owner_column.to_string(), owner.to_string());
    }
    SyncRecord::new(id, SyncMeta::new(0, 0).tombstoned(), payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_db::Filter;
    use fundamental_macros::Model;
    use fundamental_sync::SliceStore;
    use fundamental_sync::wire::SliceCursor;
    use std::sync::Mutex;

    #[derive(Clone, Debug, Model)]
    #[model(offline, syncable, table = "channel_summaries")]
    struct ChannelSummary {
        #[model(id)]
        id: String,
        #[model(owner)]
        channel_id: String,
        message_count: i64,
    }

    type SeenUpsert = (String, Vec<String>, String, SyncRecord);

    #[derive(Default)]
    struct RecordingStore {
        seen: Mutex<Option<SeenUpsert>>,
        fail: bool,
    }

    impl SliceStore for RecordingStore {
        async fn changed_page(
            &self,
            _table: &str,
            _data_columns: &[&str],
            _key_columns: &[&str],
            _filters: &[Filter],
            _since: i64,
            _after: Option<&SliceCursor>,
            _limit: u32,
        ) -> Result<Vec<SyncRecord>, SyncError> {
            Ok(Vec::new())
        }
    }

    impl SliceWriteStore for RecordingStore {
        async fn upsert(
            &self,
            table: &str,
            data_columns: &[&str],
            key_columns: &[&str],
            record: &SyncRecord,
        ) -> Result<(), SyncError> {
            if self.fail {
                return Err(SyncError::Storage("disk full".to_string()));
            }
            *self.seen.lock().unwrap() = Some((
                table.to_string(),
                data_columns.iter().map(|c| c.to_string()).collect(),
                key_columns.iter().map(|c| (*c).to_string()).collect(),
                record.clone(),
            ));
            Ok(())
        }
    }

    #[tokio::test]
    async fn write_forwards_the_projection_metadata_and_record() {
        let store = Arc::new(RecordingStore::default());
        let writer = ProjectionWriter::new(store.clone());
        let record = SyncRecord::new(
            "c-1",
            SyncMeta::new(5, 2),
            BTreeMap::from([
                ("channel_id".to_string(), "c-1".to_string()),
                ("message_count".to_string(), "42".to_string()),
            ]),
        );

        writer
            .write::<ChannelSummary>(&record)
            .await
            .expect("write succeeds");

        let (table, columns, pk, seen) = store.seen.lock().unwrap().clone().expect("store called");
        assert_eq!(table, "channel_summaries");
        assert_eq!(columns, vec!["channel_id", "message_count"]);
        assert_eq!(pk, "id");
        assert_eq!(seen, record);
    }

    #[tokio::test]
    async fn tombstone_flags_the_record_and_carries_the_owner() {
        let store = Arc::new(RecordingStore::default());
        let writer = ProjectionWriter::new(store.clone());

        writer
            .tombstone::<ChannelSummary>("c-9", "c-9")
            .await
            .expect("tombstone succeeds");

        let (table, _columns, _pk, seen) =
            store.seen.lock().unwrap().clone().expect("store called");
        assert_eq!(table, "channel_summaries");
        assert_eq!(seen.id, "c-9");
        assert!(seen.meta.tombstone, "the record is flagged deleted");
        assert_eq!(
            seen.payload.get("channel_id").map(String::as_str),
            Some("c-9"),
            "the owner column is populated so the delete stays scoped"
        );
    }

    #[tokio::test]
    async fn a_store_failure_propagates_as_its_sync_error() {
        let store = Arc::new(RecordingStore {
            fail: true,
            ..Default::default()
        });
        let writer = ProjectionWriter::new(store);
        let record = SyncRecord::new("c-1", SyncMeta::new(1, 1), BTreeMap::new());

        let err = writer
            .write::<ChannelSummary>(&record)
            .await
            .expect_err("the store failed");
        assert!(matches!(err, SyncError::Storage(_)));
    }
}
