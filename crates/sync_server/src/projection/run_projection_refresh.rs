use std::sync::Arc;

use fundamental_sync::{ChangeError, ChangeStream, SliceReader, SliceWriteStore};
use tracing::{debug, warn};

use crate::projection::{ProjectionRegistry, ProjectionWriter};

/// Drives `stream`, recomputing every projection a change affects into `store`, until
/// the change publisher is dropped.
///
/// Exactly one process may run it; across nodes use
/// [`nats_projection_consumer`](crate::nats_projection_consumer). Changes dropped on lag are not
/// re-derived.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub async fn run_projection_refresh<S>(
    mut stream: ChangeStream,
    registry: Arc<ProjectionRegistry>,
    store: Arc<S>,
) where
    S: SliceWriteStore + Send + Sync + 'static,
{
    let writer = ProjectionWriter::new(store.clone());
    let reader: Arc<dyn SliceReader> = store;
    loop {
        match stream.recv().await {
            Ok(change) => registry.apply(&change, &reader, &writer).await,
            Err(ChangeError::Lagged(skipped)) => warn!(
                skipped,
                "projection refresh lagged; affected projection rows may be stale until \
                 their sources change again or a reconciliation runs"
            ),
            Err(_) => break,
        }
    }
    debug!("projection refresh stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_db::Filter;
    use fundamental_macros::Model;
    use fundamental_sync::change::{BroadcastChangePublisher, ChangePublisher};
    use fundamental_sync::wire::SliceCursor;
    use fundamental_sync::{SliceStore, SyncChange, SyncError, SyncMeta, SyncRecord};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use crate::ProjectionUpdate;

    #[derive(Clone, Debug, Model)]
    #[model(offline, syncable, table = "channel_summaries")]
    struct ChannelSummary {
        #[model(id)]
        id: String,
        #[model(owner)]
        channel_id: String,
    }

    #[derive(Clone, Debug, Model)]
    #[model(offline, syncable, table = "messages")]
    struct Message {
        #[model(id)]
        id: String,
        #[model(owner)]
        channel_id: String,
    }

    #[derive(Default)]
    struct RecordingStore {
        writes: Mutex<Vec<SyncRecord>>,
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
            _table: &str,
            _data_columns: &[&str],
            _key_columns: &[&str],
            record: &SyncRecord,
        ) -> Result<(), SyncError> {
            self.writes.lock().unwrap().push(record.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn the_loop_recomputes_published_changes_then_stops_when_the_publisher_drops() {
        let registry = Arc::new(
            ProjectionRegistry::new().define::<ChannelSummary, Message, _, _>(
                "channel_summary",
                |_ctx, change| async move {
                    Ok(vec![ProjectionUpdate::upsert(SyncRecord::new(
                        change.owner.clone(),
                        SyncMeta::new(0, 0),
                        BTreeMap::from([("channel_id".to_string(), change.owner)]),
                    ))])
                },
            ),
        );
        let store = Arc::new(RecordingStore::default());
        let publisher = BroadcastChangePublisher::new(16);
        let handle = tokio::spawn(run_projection_refresh(
            publisher.subscribe(),
            registry,
            store.clone(),
        ));

        for channel in ["c-1", "c-2"] {
            publisher
                .publish(SyncChange::new(
                    channel,
                    "messages",
                    SyncRecord::new("m", SyncMeta::new(1, 1), BTreeMap::new()),
                    1,
                ))
                .await
                .expect("publish");
        }

        drop(publisher);
        handle.await.expect("loop task joins");

        let mut written: Vec<String> = store
            .writes
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.id.to_string())
            .collect();
        written.sort();
        assert_eq!(written, vec!["c-1".to_string(), "c-2".to_string()]);
    }
}
