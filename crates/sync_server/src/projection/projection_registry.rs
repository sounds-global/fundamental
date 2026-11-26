use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use fundamental_sync::{
    CaptureTable, ServerSyncable, SingleKeySyncable, SliceReader, SliceWriteStore, SyncChange,
};
use tracing::{debug, instrument, warn};

use crate::projection::projection_writer::tombstone_record;
use crate::projection::{ProjectionContext, ProjectionError, ProjectionUpdate, ProjectionWriter};

type RecomputeFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<ProjectionUpdate>, ProjectionError>> + Send + 'a>>;

/// The server's registry of projection recompute handlers, keyed by source table.
///
/// [`define`](Self::define) registers, per projection `P` and source `Src`, a handler mapping a
/// change to `Src` into the `P` rows to rewrite or tombstone.
///
/// ```
/// # extern crate fundamental_sync_server;
/// # extern crate fundamental_sync;
/// # extern crate fundamental_macros;
/// use fundamental_macros::Model;
/// use fundamental_sync_server::{ProjectionRegistry, ProjectionUpdate};
///
/// #[derive(Clone, Debug, Model)]
/// #[model(offline, syncable, table = "channel_summaries")]
/// struct ChannelSummary {
///     #[model(id)]
///     id: String,
///     #[model(owner)]
///     channel_id: String,
///     message_count: i64,
/// }
///
/// #[derive(Clone, Debug, Model)]
/// #[model(offline, syncable, table = "messages")]
/// struct Message {
///     #[model(id)]
///     id: String,
///     #[model(owner)]
///     channel_id: String,
/// }
///
/// let registry = ProjectionRegistry::new().define::<ChannelSummary, Message, _, _>(
///     "channel_summary",
///     |ctx, change| async move {
///         // Recompute the summary for the channel the changed message belongs to.
///         let _ = (ctx, change);
///         Ok::<_, fundamental_sync_server::ProjectionError>(Vec::<ProjectionUpdate>::new())
///     },
/// );
/// assert_eq!(registry.source_capture_tables().len(), 1);
/// ```
#[derive(Default)]
#[must_use]
pub struct ProjectionRegistry {
    by_source: HashMap<&'static str, Vec<Box<dyn ProjectionResolver>>>,
}

impl ProjectionRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a `name`d handler that recomputes projection `P` when source `Src` changes.
    pub fn define<P, Src, F, Fut>(mut self, name: impl Into<String>, closure: F) -> Self
    where
        P: ServerSyncable + Send + Sync + 'static,
        // A composite-keyed source would recompute from one row of a whole partition.
        Src: SingleKeySyncable + Send + Sync + 'static,
        F: Fn(ProjectionContext, SyncChange) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<ProjectionUpdate>, ProjectionError>> + Send,
    {
        self.by_source
            .entry(Src::TABLE)
            .or_default()
            .push(Box::new(DefinedProjection {
                name: name.into(),
                closure,
                _marker: PhantomData::<fn(P, Src)>,
            }));
        self
    }

    /// Whether any handler is registered for changes to source `table`.
    #[must_use]
    pub fn watches(&self, table: &str) -> bool {
        self.by_source.contains_key(table)
    }

    /// The [`CaptureTable`] for every distinct source table with an owner column.
    ///
    /// Union it with [`capture_tables`](crate::SliceRegistry::capture_tables) through
    /// [`merge_capture_tables`](crate::merge_capture_tables).
    #[must_use]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn source_capture_tables(&self) -> Vec<CaptureTable> {
        let mut seen = HashMap::new();
        for resolvers in self.by_source.values() {
            for resolver in resolvers {
                if let Some(capture) = resolver.source_capture() {
                    seen.entry(resolver.source_table()).or_insert(capture);
                }
            }
        }
        seen.into_values().collect()
    }

    /// Recomputes every projection affected by `change`, writing the results through
    /// `writer`; a failure is logged and skipped.
    ///
    /// Records the source table only: a row's id and owner may come from a client.
    #[instrument(
        name = "sync.projection.apply",
        skip_all,
        fields(source_table = %change.table)
    )]
    pub(crate) async fn apply<S>(
        &self,
        change: &SyncChange,
        reader: &Arc<dyn SliceReader>,
        writer: &ProjectionWriter<S>,
    ) where
        S: SliceWriteStore + Send + Sync + 'static,
    {
        let Some(resolvers) = self.by_source.get(change.table.as_str()) else {
            return;
        };
        let ctx = ProjectionContext::new(reader.clone());
        for resolver in resolvers {
            let updates = match resolver.recompute(ctx.clone(), change.clone()).await {
                Ok(updates) => updates,
                Err(error) => {
                    warn!(
                        projection = %resolver.name(),
                        error = %error,
                        "projection recompute failed; skipping this change"
                    );
                    continue;
                }
            };

            let mut rewritten = 0usize;
            for update in updates {
                let record = match &update {
                    ProjectionUpdate::Upsert(record) => record.clone(),
                    ProjectionUpdate::Tombstone { id, owner } => {
                        tombstone_record(resolver.projection_owner_column(), id, owner)
                    }
                };
                match writer
                    .write_record(
                        resolver.projection_table(),
                        resolver.projection_columns(),
                        resolver.projection_key_columns(),
                        &record,
                    )
                    .await
                {
                    Ok(()) => rewritten += 1,
                    Err(error) => warn!(
                        projection = %resolver.projection_table(),
                        error = %error,
                        "projection write failed"
                    ),
                }
            }
            debug!(
                projection = %resolver.name(),
                rows_rewritten = rewritten,
                "projection refreshed"
            );
        }
    }
}

trait ProjectionResolver: Send + Sync {
    fn name(&self) -> &str;

    fn source_table(&self) -> &'static str;

    fn projection_table(&self) -> &'static str;

    fn projection_columns(&self) -> &'static [&'static str];

    fn projection_key_columns(&self) -> &'static [&'static str];

    fn projection_owner_column(&self) -> Option<&'static str>;

    fn source_capture(&self) -> Option<CaptureTable>;

    fn recompute(&self, ctx: ProjectionContext, change: SyncChange) -> RecomputeFuture<'_>;
}

struct DefinedProjection<P, Src, F> {
    name: String,
    closure: F,
    _marker: PhantomData<fn(P, Src)>,
}

impl<P, Src, F, Fut> ProjectionResolver for DefinedProjection<P, Src, F>
where
    P: ServerSyncable + Send + Sync + 'static,
    Src: SingleKeySyncable + Send + Sync + 'static,
    F: Fn(ProjectionContext, SyncChange) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<ProjectionUpdate>, ProjectionError>> + Send,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn source_table(&self) -> &'static str {
        Src::TABLE
    }

    fn projection_table(&self) -> &'static str {
        P::TABLE
    }

    fn projection_columns(&self) -> &'static [&'static str] {
        P::data_columns()
    }

    fn projection_key_columns(&self) -> &'static [&'static str] {
        P::KEY_COLUMNS
    }

    fn projection_owner_column(&self) -> Option<&'static str> {
        P::OWNER_COLUMN
    }

    fn source_capture(&self) -> Option<CaptureTable> {
        Src::OWNER_COLUMN.map(|owner| CaptureTable::for_model::<Src>(owner))
    }

    fn recompute(&self, ctx: ProjectionContext, change: SyncChange) -> RecomputeFuture<'_> {
        Box::pin(async move { (self.closure)(ctx, change).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_db::Filter;
    use fundamental_macros::Model;
    use fundamental_sync::wire::SliceCursor;
    use fundamental_sync::{SliceStore, SyncError, SyncMeta, SyncRecord};
    use std::collections::BTreeMap;
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

    #[derive(Clone, Debug, Model)]
    #[model(offline, syncable, table = "messages")]
    struct Message {
        #[model(id)]
        id: String,
        #[model(owner)]
        channel_id: String,
    }

    #[derive(Clone, Debug, Model)]
    #[model(offline, syncable, table = "flags")]
    struct Flag {
        #[model(id)]
        id: String,
        note: String,
    }

    #[derive(Default)]
    struct RecordingStore {
        writes: Mutex<Vec<(String, SyncRecord)>>,
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
            _data_columns: &[&str],
            _key_columns: &[&str],
            record: &SyncRecord,
        ) -> Result<(), SyncError> {
            self.writes
                .lock()
                .unwrap()
                .push((table.to_string(), record.clone()));
            Ok(())
        }
    }

    fn change(table: &str, id: &str, owner: &str) -> SyncChange {
        SyncChange::new(
            owner,
            table,
            SyncRecord::new(
                id,
                SyncMeta::new(1, 1),
                BTreeMap::from([("channel_id".to_string(), owner.to_string())]),
            ),
            1,
        )
    }

    fn setup(
        store: Arc<RecordingStore>,
    ) -> (Arc<dyn SliceReader>, ProjectionWriter<RecordingStore>) {
        let reader: Arc<dyn SliceReader> = store.clone();
        (reader, ProjectionWriter::new(store))
    }

    #[tokio::test]
    async fn a_source_change_recomputes_and_writes_the_projection() {
        let registry = ProjectionRegistry::new().define::<ChannelSummary, Message, _, _>(
            "channel_summary",
            |_ctx, change| async move {
                let channel = change.owner.clone();
                Ok(vec![ProjectionUpdate::upsert(SyncRecord::new(
                    &channel,
                    SyncMeta::new(0, 0),
                    BTreeMap::from([
                        ("channel_id".to_string(), channel.clone()),
                        ("message_count".to_string(), "1".to_string()),
                    ]),
                ))])
            },
        );
        let store = Arc::new(RecordingStore::default());
        let (reader, writer) = setup(store.clone());

        registry
            .apply(&change("messages", "m-1", "c-7"), &reader, &writer)
            .await;

        let writes = store.writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, "channel_summaries");
        assert_eq!(writes[0].1.id, "c-7");
        assert_eq!(
            writes[0].1.payload.get("message_count").map(String::as_str),
            Some("1")
        );
    }

    #[tokio::test]
    async fn a_tombstone_update_writes_a_scoped_tombstone_record() {
        let registry = ProjectionRegistry::new().define::<ChannelSummary, Message, _, _>(
            "channel_summary",
            |_ctx, change| async move {
                Ok(vec![ProjectionUpdate::tombstone(
                    change.owner.clone(),
                    change.owner,
                )])
            },
        );
        let store = Arc::new(RecordingStore::default());
        let (reader, writer) = setup(store.clone());

        registry
            .apply(&change("messages", "m-1", "c-3"), &reader, &writer)
            .await;

        let writes = store.writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert!(writes[0].1.meta.tombstone);
        assert_eq!(
            writes[0].1.payload.get("channel_id").map(String::as_str),
            Some("c-3"),
            "the tombstone carries the projection owner column"
        );
    }

    #[tokio::test]
    async fn an_unwatched_source_change_is_ignored() {
        let registry = ProjectionRegistry::new().define::<ChannelSummary, Message, _, _>(
            "channel_summary",
            |_ctx, _change| async move { Ok(Vec::new()) },
        );
        let store = Arc::new(RecordingStore::default());
        let (reader, writer) = setup(store.clone());

        registry
            .apply(&change("animals", "a-1", "c-1"), &reader, &writer)
            .await;

        assert!(store.writes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_handler_error_is_skipped_and_the_other_handlers_still_run() {
        let registry = ProjectionRegistry::new()
            .define::<ChannelSummary, Message, _, _>("failing", |_ctx, _change| async move {
                Err(ProjectionError::Read("boom".to_string()))
            })
            .define::<ChannelSummary, Message, _, _>("working", |_ctx, change| async move {
                Ok(vec![ProjectionUpdate::upsert(SyncRecord::new(
                    change.owner.clone(),
                    SyncMeta::new(0, 0),
                    BTreeMap::from([("channel_id".to_string(), change.owner)]),
                ))])
            });
        let store = Arc::new(RecordingStore::default());
        let (reader, writer) = setup(store.clone());

        registry
            .apply(&change("messages", "m-1", "c-1"), &reader, &writer)
            .await;

        assert_eq!(store.writes.lock().unwrap().len(), 1);
    }

    #[test]
    fn source_capture_tables_cover_owned_sources_once_and_skip_unowned() {
        let registry = ProjectionRegistry::new()
            .define::<ChannelSummary, Message, _, _>("a", |_c, _ch| async move { Ok(Vec::new()) })
            .define::<ChannelSummary, Message, _, _>("b", |_c, _ch| async move { Ok(Vec::new()) })
            .define::<ChannelSummary, Flag, _, _>("c", |_c, _ch| async move { Ok(Vec::new()) });

        let captures = registry.source_capture_tables();
        assert_eq!(captures.len(), 1, "{captures:?}");
        assert_eq!(captures[0].table(), "messages");
        assert_eq!(captures[0].owner_column(), "channel_id");
    }
}
