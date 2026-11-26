use std::sync::Arc;

use fundamental_auth::AuthenticatedPrincipal;
use fundamental_core::Error;
use fundamental_sync::wire::SliceCursor;
use fundamental_sync::{
    SliceReader, SliceStore, SliceWriteStore, SyncError, SyncRecord, record_matches,
};
use serde_json::Value;
use tracing::{Span, error, field, info, instrument, warn};

use crate::SliceRegistry;
use crate::slice::{ResolvedSlice, SliceError};

/// Ties a [`SliceRegistry`] to a backend [`SliceStore`]: it resolves a named,
/// parameterized subscription into a scoped query and reads its rows.
pub struct SliceServerSource<S> {
    registry: SliceRegistry,
    store: Arc<S>,
}

impl<S: SliceStore + Send + Sync + 'static> SliceServerSource<S> {
    /// Combines a slice registry with a backend store.
    pub fn new(registry: SliceRegistry, store: S) -> Self {
        Self {
            registry,
            store: Arc::new(store),
        }
    }

    /// Combines a slice registry with a shared store, so the same instance can drive projection
    /// refresh.
    pub fn from_arc(registry: SliceRegistry, store: Arc<S>) -> Self {
        Self { registry, store }
    }

    /// Resolves `slice` for `principal` with `params` into its scoped query, without reading.
    ///
    /// # Errors
    /// Returns the [`SliceError`] if the slice is unknown, its parameters are
    /// invalid, or the principal is not authorized for it; the refusal is logged here.
    #[instrument(name = "sync.slice.subscribe", skip_all)]
    pub async fn resolve(
        &self,
        principal: &AuthenticatedPrincipal,
        slice: &str,
        params: Value,
    ) -> Result<ResolvedSlice, SliceError> {
        let reader: Arc<dyn SliceReader> = self.store.clone();
        let resolved = self
            .registry
            .resolve(slice, principal, params, reader)
            .await;
        if let Err(error) = &resolved {
            record_failure(error);
        }
        resolved
    }

    /// Resolves `slice` and returns one page of its rows: up to `limit` records after `after`
    /// (or after `since` without one), plus the cursor to resume from when more remain.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the slice is unknown, its parameters are
    /// invalid, the principal is unauthorized, or the store read fails.
    ///
    /// The span never records the cursor, `since` or a record id, which are the client's.
    #[instrument(
        name = "sync.slice.pull",
        skip_all,
        fields(slice = field::Empty, limit, has_cursor = after.is_some())
    )]
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub async fn pull(
        &self,
        principal: &AuthenticatedPrincipal,
        slice: &str,
        params: Value,
        since: i64,
        after: Option<&SliceCursor>,
        limit: u32,
    ) -> Result<(Vec<SyncRecord>, Option<SliceCursor>), SyncError> {
        let page = self
            .pull_page(principal, slice, params, since, after, limit)
            .await;
        match page {
            Ok((records, next)) => {
                info!(
                    rows = records.len(),
                    has_more = next.is_some(),
                    "slice pull served"
                );
                Ok((records, next))
            }
            Err(failure) => Err(failure.recorded()),
        }
    }

    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    async fn pull_page(
        &self,
        principal: &AuthenticatedPrincipal,
        slice: &str,
        params: Value,
        since: i64,
        after: Option<&SliceCursor>,
        limit: u32,
    ) -> Result<(Vec<SyncRecord>, Option<SliceCursor>), SliceFailure> {
        let reader: Arc<dyn SliceReader> = self.store.clone();
        let resolved = self
            .registry
            .resolve(slice, principal, params, reader)
            .await
            .map_err(|e| SliceFailure::resolving(&e, SyncError::Pull))?;
        Span::current().record("slice", &*resolved.name);

        let over = limit.saturating_add(1);
        let mut records = self
            .store
            .changed_page(
                resolved.table,
                resolved.data_columns,
                resolved.key_columns,
                &[resolved.filter],
                since,
                after,
                over,
            )
            .await
            .map_err(SliceFailure::Failed)?;

        let next = if records.len() as u64 > u64::from(limit) {
            records.truncate(limit as usize);
            records
                .last()
                .map(|record| SliceCursor::new(record.meta.updated_at, record.id.clone()))
        } else {
            None
        };
        Ok((records, next))
    }
}

impl<S: SliceWriteStore + Send + Sync + 'static> SliceServerSource<S> {
    /// Applies a client `record` pushed to `slice`, refused unless the table is bidirectional
    /// and the record is in the slice's scope.
    ///
    /// # Errors
    /// Returns [`SyncError::Push`] if the slice does not resolve, the table is pull-only, or the
    /// record is out of scope; or the store's error.
    #[instrument(name = "sync.slice.push", skip_all, fields(slice = field::Empty))]
    pub async fn push(
        &self,
        principal: &AuthenticatedPrincipal,
        slice: &str,
        params: Value,
        record: &SyncRecord,
    ) -> Result<(), SyncError> {
        match self.push_record(principal, slice, params, record).await {
            Ok(()) => {
                info!("slice upsert applied");
                Ok(())
            }
            Err(failure) => Err(failure.recorded()),
        }
    }

    async fn push_record(
        &self,
        principal: &AuthenticatedPrincipal,
        slice: &str,
        params: Value,
        record: &SyncRecord,
    ) -> Result<(), SliceFailure> {
        let reader: Arc<dyn SliceReader> = self.store.clone();
        let resolved = self
            .registry
            .resolve(slice, principal, params, reader)
            .await
            .map_err(|e| SliceFailure::resolving(&e, SyncError::Push))?;
        Span::current().record("slice", &*resolved.name);

        if !resolved.bidirectional {
            return Err(SliceFailure::Refused(SyncError::Push(format!(
                "slice `{}` is pull-only",
                resolved.name
            ))));
        }
        if !record_matches(&resolved.filter, record, resolved.key_columns) {
            return Err(SliceFailure::Refused(SyncError::Push(format!(
                "the record is outside slice `{}`",
                resolved.name
            ))));
        }

        self.store
            .upsert(
                resolved.table,
                resolved.data_columns,
                resolved.key_columns,
                record,
            )
            .await
            .map_err(SliceFailure::Failed)
    }
}

/// A pull or push that did not complete, tagged by whose doing it was.
///
/// The tag decides only the log level, so a client's mistake never pages an operator.
enum SliceFailure {
    /// Refused because of what the client sent.
    Refused(SyncError),
    /// Failed on the server's side.
    Failed(SyncError),
}

impl SliceFailure {
    /// A slice that did not resolve, tagged by the slice error's status.
    fn resolving(error: &SliceError, wrap: fn(String) -> SyncError) -> Self {
        let reported = wrap(error.to_string());
        if error.http_status_code() >= 500 {
            Self::Failed(reported)
        } else {
            Self::Refused(reported)
        }
    }

    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn recorded(self) -> SyncError {
        match self {
            Self::Refused(refused) => {
                warn!(code = refused.error_code(), error = %refused, "slice operation refused");
                refused
            }
            Self::Failed(failed) => {
                error!(code = failed.error_code(), error = %failed, "slice operation failed");
                failed
            }
        }
    }
}

/// Logs a slice operation once: a client refusal at `warn`, a server failure at `error`.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn record_failure<E: Error>(failure: &E) {
    if failure.http_status_code() >= 500 {
        error!(code = failure.error_code(), error = %failure, "slice operation failed");
    } else {
        warn!(code = failure.error_code(), error = %failure, "slice operation refused");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoParams;
    use fundamental_auth::UserId;
    use fundamental_db::{Filter, FilterCondition, RecordKey};
    use fundamental_macros::Model;
    use fundamental_sync::SyncMeta;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[derive(Clone, Debug, Model)]
    #[model(offline, syncable, table = "animals")]
    struct Animal {
        #[model(id)]
        id: String,
        name: String,
        owner_id: String,
    }

    type SeenCall = (String, Vec<String>, String, Vec<Filter>, i64, u32);

    #[derive(Default)]
    struct FakeStore {
        seen: Mutex<Option<SeenCall>>,
    }

    impl SliceStore for FakeStore {
        async fn changed_page(
            &self,
            table: &str,
            data_columns: &[&str],
            key_columns: &[&str],
            filters: &[Filter],
            since: i64,
            _after: Option<&SliceCursor>,
            limit: u32,
        ) -> Result<Vec<SyncRecord>, SyncError> {
            *self.seen.lock().unwrap() = Some((
                table.to_string(),
                data_columns.iter().map(|c| c.to_string()).collect(),
                key_columns.iter().map(|c| (*c).to_string()).collect(),
                filters.to_vec(),
                since,
                limit,
            ));
            Ok(vec![SyncRecord {
                id: RecordKey::single("row-1"),
                meta: SyncMeta::new(42, 1),
                payload: BTreeMap::new(),
            }])
        }
    }

    fn principal(user: &str) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            user_id: UserId::new(user),
            device_id: None,
            scopes: Vec::new(),
            expires_at: 0,
        }
    }

    #[tokio::test]
    async fn pull_resolves_the_slice_and_forwards_it_to_the_store() {
        let registry = SliceRegistry::new()
            .define::<Animal, NoParams, _, _>("mine", |ctx, _params| async move {
                Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
            });
        let source = SliceServerSource::new(registry, FakeStore::default());

        let (records, next) = source
            .pull(&principal("alice"), "mine", Value::Null, 7, None, 100)
            .await
            .expect("pull succeeds");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "row-1");
        assert!(next.is_none(), "one row under the limit is the final page");

        let (table, columns, pk, filters, since, limit) = source
            .store
            .seen
            .lock()
            .unwrap()
            .clone()
            .expect("store called");
        assert_eq!(table, "animals");
        assert_eq!(columns, vec!["name", "owner_id"]);
        assert_eq!(pk, "id");
        assert_eq!(since, 7);
        assert_eq!(
            limit, 101,
            "the source over-fetches by one to detect more pages"
        );
        assert!(matches!(
            &filters[0].condition,
            FilterCondition::Eq { column, .. } if column == "owner_id"
        ));
    }

    #[tokio::test]
    async fn from_arc_shares_the_store_and_resolves_like_new() {
        let registry = SliceRegistry::new()
            .define::<Animal, NoParams, _, _>("mine", |ctx, _params| async move {
                Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
            });
        let store = Arc::new(FakeStore::default());
        let source = SliceServerSource::from_arc(registry, Arc::clone(&store));

        let (records, _next) = source
            .pull(&principal("alice"), "mine", Value::Null, 7, None, 100)
            .await
            .expect("pull succeeds");
        assert_eq!(records.len(), 1);
        assert!(
            store.seen.lock().unwrap().is_some(),
            "the shared store instance was queried"
        );
    }

    #[tokio::test]
    async fn pull_surfaces_an_unknown_slice_as_a_pull_error() {
        let source = SliceServerSource::new(SliceRegistry::new(), FakeStore::default());
        let err = source
            .pull(&principal("alice"), "ghost", Value::Null, 0, None, 100)
            .await
            .expect_err("unknown slice");
        assert!(matches!(err, SyncError::Pull(_)));
        assert!(
            source.store.seen.lock().unwrap().is_none(),
            "store not called"
        );
    }
}
