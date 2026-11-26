use std::future::Future;
use std::sync::Arc;

use fundamental_auth::PrincipalVerifier;
use fundamental_core::AnyError;
use fundamental_runtime::LiveSyncSurface;
use fundamental_sync::change::BroadcastChangePublisher;
use fundamental_sync::{CaptureTable, SliceWriteStore, SyncError};
use fundamental_ws::{WsConfig, WsServer};
use tracing::{info, instrument};

use crate::{
    ProjectionRegistry, RateLimiter, SliceRegistry, SliceServerSource, live_slice_sync_router,
    merge_capture_tables, run_projection_refresh, slice_sync_router,
};

/// Builds a live slice-sync server as a supervised [`LiveSyncSurface`] in one call.
///
/// The caller supplies change capture to [`serve_with_capture`](Self::serve_with_capture); a
/// closure returning no handles makes [`SyncError::NoChangeCapture`] refuse at wiring time.
///
/// [`SyncError::NoChangeCapture`]: fundamental_sync::SyncError::NoChangeCapture
///
/// # Examples
/// ```rust
/// use std::sync::Arc;
///
/// use fundamental_sync_server::{LiveSyncServer, ProjectionRegistry, SliceRegistry};
/// use fundamental_ws::WsConfig;
/// # use fundamental_auth::{AuthError, AuthenticatedPrincipal, PrincipalVerifier};
/// # use fundamental_core::secrets::SecretString;
/// # use fundamental_db::Filter;
/// # use fundamental_sync::wire::SliceCursor;
/// # use fundamental_sync::{SliceStore, SliceWriteStore, SyncError, SyncRecord};
/// # struct NoAuth;
/// # impl PrincipalVerifier for NoAuth {
/// #     async fn verify(&self, _t: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
/// #         unreachable!()
/// #     }
/// # }
/// # struct MemStore;
/// # impl SliceStore for MemStore {
/// #     async fn changed_page(&self, _t: &str, _d: &[&str], _pk: &[&str], _f: &[Filter],
/// #         _s: i64, _a: Option<&SliceCursor>, _l: u32) -> Result<Vec<SyncRecord>, SyncError> {
/// #         Ok(Vec::new())
/// #     }
/// # }
/// # impl SliceWriteStore for MemStore {
/// #     async fn upsert(&self, _t: &str, _d: &[&str], _pk: &[&str], _r: &SyncRecord)
/// #         -> Result<(), SyncError> { Ok(()) }
/// # }
/// # async fn run() -> Result<(), fundamental_core::AnyError> {
/// let store = Arc::new(MemStore);
/// let verifier = Arc::new(NoAuth);
/// let config = WsConfig::new("127.0.0.1:9100".parse().unwrap());
///
/// // With no slices or projections registered there is nothing to capture, so this
/// // serves pull-only and the capture closure is never called.
/// let surface = LiveSyncServer::new(
///     SliceRegistry::new(),
///     ProjectionRegistry::new(),
///     store,
///     verifier,
///     config,
/// )
/// .serve_with_capture(|_publisher, _capture| async move { Ok(Vec::new()) })
/// .await?;
/// # let _ = surface;
/// # Ok(())
/// # }
/// ```
#[must_use = "a LiveSyncServer does nothing until `serve_with_capture` is called"]
pub struct LiveSyncServer<V, S> {
    registry: SliceRegistry,
    projections: ProjectionRegistry,
    store: Arc<S>,
    verifier: Arc<V>,
    config: WsConfig,
    limiter: Arc<RateLimiter>,
    publisher_capacity: usize,
}

impl<V, S> LiveSyncServer<V, S>
where
    V: PrincipalVerifier + Send + Sync + 'static,
    S: SliceWriteStore + Send + Sync + 'static,
{
    /// Starts a builder from the served slices and projections, the shared backing
    /// `store`, the principal `verifier`, and the WebSocket `config` (bind address).
    ///
    /// Defaults: a [`RateLimiter`] of 1000 tokens at 1000/s and a publisher buffer of 1024.
    pub fn new(
        registry: SliceRegistry,
        projections: ProjectionRegistry,
        store: Arc<S>,
        verifier: Arc<V>,
        config: WsConfig,
    ) -> Self {
        Self {
            registry,
            projections,
            store,
            verifier,
            config,
            limiter: Arc::new(RateLimiter::new(1000, 1000.0, 65_536)),
            publisher_capacity: 1024,
        }
    }

    /// Overrides the per-principal rate limiter.
    pub fn limiter(mut self, limiter: Arc<RateLimiter>) -> Self {
        self.limiter = limiter;
        self
    }

    /// Overrides the change-publisher buffer capacity; a slow consumer that overflows drops
    /// changes.
    pub fn publisher_capacity(mut self, capacity: usize) -> Self {
        self.publisher_capacity = capacity;
        self
    }

    /// Wires the surface: with anything captured it builds the publisher, calls
    /// `start_capture`, spawns projection refresh and serves the live router; otherwise it
    /// serves pull-only and never calls `start_capture`.
    ///
    /// The handles `start_capture` returns are dropped before the publisher on shutdown.
    ///
    /// # Errors
    /// Returns the [`AnyError`] from `start_capture` if beginning change capture fails.
    #[instrument(skip_all, err)]
    pub async fn serve_with_capture<F, Fut>(
        self,
        start_capture: F,
    ) -> Result<LiveSyncSurface, AnyError>
    where
        F: FnOnce(BroadcastChangePublisher, Vec<CaptureTable>) -> Fut,
        Fut: Future<Output = Result<Vec<Box<dyn Send>>, AnyError>>,
    {
        let capture = merge_capture_tables(
            self.registry
                .capture_tables()
                .into_iter()
                .chain(self.projections.source_capture_tables()),
        );

        // Every served table must exist, so a missing migration surfaces here with its name.
        let mut hub_tables: std::collections::BTreeSet<&str> =
            self.registry.slice_tables().into_iter().collect();
        hub_tables.extend(capture.iter().map(CaptureTable::table));
        for table in hub_tables {
            if !self
                .store
                .table_exists(table)
                .await
                .map_err(AnyError::new)?
            {
                return Err(AnyError::new(SyncError::UnprovisionedHub {
                    table: table.to_string(),
                }));
            }
        }

        let source = Arc::new(SliceServerSource::from_arc(
            self.registry,
            self.store.clone(),
        ));

        let (router, keep_alive, projection) = if capture.is_empty() {
            info!("no capture tables: serving slice-sync pull-only (no live fan-out)");
            (
                slice_sync_router(self.verifier, source, self.limiter),
                Vec::new(),
                None,
            )
        } else {
            let capture_tables = capture.len();
            let publisher = BroadcastChangePublisher::new(self.publisher_capacity);
            // The publisher goes last, so shutdown stops capture before closing the stream.
            let mut keep_alive = start_capture(publisher.clone(), capture).await?;
            let projection = tokio::spawn(run_projection_refresh(
                publisher.subscribe(),
                Arc::new(self.projections),
                self.store,
            ));
            let router = live_slice_sync_router(self.verifier, source, self.limiter, &publisher);
            keep_alive.push(Box::new(publisher));
            info!(
                capture_tables,
                "serving live slice-sync with change capture"
            );
            (router, keep_alive, Some(projection))
        };

        Ok(LiveSyncSurface::new(
            WsServer::new(self.config, router),
            keep_alive,
            projection,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoParams;
    use fundamental_auth::{AuthError, AuthenticatedPrincipal, PrincipalVerifier};
    use fundamental_core::secrets::SecretString;
    use fundamental_db::Filter;
    use fundamental_macros::Model;
    use fundamental_sync::wire::SliceCursor;
    use fundamental_sync::{SliceStore, SliceWriteStore, SyncRecord};

    #[derive(Clone, Debug, Model)]
    #[model(offline, syncable, table = "widgets")]
    struct Widget {
        #[model(id)]
        id: String,
        #[model(owner)]
        owner_id: String,
    }

    #[derive(Clone, Debug, Model)]
    #[model(offline, syncable, table = "gadgets")]
    struct Gadget {
        #[model(id)]
        id: String,
        label: String,
    }

    struct NoAuth;
    impl PrincipalVerifier for NoAuth {
        async fn verify(&self, _t: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
            unreachable!("wiring never authenticates in these tests")
        }
    }

    /// A store that reports its hub table missing.
    struct MissingHubStore;
    impl SliceStore for MissingHubStore {
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
        async fn table_exists(&self, _table: &str) -> Result<bool, SyncError> {
            Ok(false)
        }
    }
    impl SliceWriteStore for MissingHubStore {
        async fn upsert(
            &self,
            _table: &str,
            _data_columns: &[&str],
            _key_columns: &[&str],
            _record: &SyncRecord,
        ) -> Result<(), SyncError> {
            Ok(())
        }
    }

    /// A store that reports every table present.
    struct PresentHubStore;
    impl SliceStore for PresentHubStore {
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
    impl SliceWriteStore for PresentHubStore {
        async fn upsert(
            &self,
            _table: &str,
            _data_columns: &[&str],
            _key_columns: &[&str],
            _record: &SyncRecord,
        ) -> Result<(), SyncError> {
            Ok(())
        }
    }

    fn config() -> WsConfig {
        WsConfig::new("127.0.0.1:0".parse().expect("addr"))
    }

    #[tokio::test]
    async fn wiring_fails_fast_when_a_served_hub_table_is_missing() {
        let registry = SliceRegistry::new()
            .define::<Widget, NoParams, _, _>("mine", |ctx, _p| async move {
                Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
            });
        let server = LiveSyncServer::new(
            registry,
            ProjectionRegistry::new(),
            Arc::new(MissingHubStore),
            Arc::new(NoAuth),
            config(),
        );
        let err = match server
            .serve_with_capture(|_p, _c| async move { Ok(Vec::new()) })
            .await
        {
            Ok(_) => panic!("a missing hub table must fail wiring"),
            Err(e) => e,
        };
        let sync_err = err.downcast_ref::<SyncError>().expect("a SyncError");
        assert!(
            matches!(sync_err, SyncError::UnprovisionedHub { table } if table == "widgets"),
            "expected UnprovisionedHub for `widgets`, got {sync_err:?}"
        );
    }

    #[tokio::test]
    async fn wiring_succeeds_when_the_store_reports_the_table_present() {
        let registry = SliceRegistry::new()
            .define::<Gadget, NoParams, _, _>("all-gadgets", |_ctx, _p| async move {
                Ok(Filter::eq("id", "any"))
            });
        let server = LiveSyncServer::new(
            registry,
            ProjectionRegistry::new(),
            Arc::new(PresentHubStore),
            Arc::new(NoAuth),
            config(),
        );
        let surface = server
            .serve_with_capture(|_p, _c| async move { Ok(Vec::new()) })
            .await
            .expect("a present hub table wires cleanly");
        let _ = surface;
    }
}
