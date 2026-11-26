use std::sync::Arc;

use fundamental_auth::JwtVerifier;
use fundamental_config::AppSettings;
use fundamental_config::settings::AuthSettings;
use fundamental_core::AnyError;
use fundamental_runtime::{DrainPhase, LiveSyncSurface};
use fundamental_sql::{SqlPool, SqlSliceStore};
use fundamental_sync::{CaptureTable, SliceWriteStore, SyncError};
use fundamental_sync_server::{LiveSyncServer, ProjectionRegistry, SliceRegistry};
use fundamental_ws::{WsConfig, WsRouter, WsServer};

use crate::deferred::{Built, Deferred};
use crate::{AppError, Served};

/// The surface name, as `[apps.<name>.sync]` declares it.
pub(crate) const SYNC: &str = "sync";

/// The database an app's sync server reads and writes its slices in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SyncStore {
    /// The app's default SQL pool.
    Sql,
    /// The app's default CQL keyspace (feature `cql`).
    #[cfg(feature = "cql")]
    Cql,
}

impl SyncStore {
    /// The store's name, as the manifest's `[apps.<name>.database.<store>]` spells it.
    const fn name(self) -> &'static str {
        match self {
            Self::Sql => "sql",
            #[cfg(feature = "cql")]
            Self::Cql => "cql",
        }
    }
}

/// What an app's [`sync`](crate::App::sync) declares.
#[derive(Default)]
pub(crate) struct LiveSync {
    slices: Option<fn(SliceRegistry) -> SliceRegistry>,
    projections: Option<fn(ProjectionRegistry) -> ProjectionRegistry>,
    store: Option<SyncStore>,
}

impl LiveSync {
    /// Whether the app declares a sync server.
    pub(crate) fn declared(&self) -> bool {
        self.slices.is_some()
    }

    /// Declares the slices `register` adds.
    pub(crate) fn slices(&mut self, register: fn(SliceRegistry) -> SliceRegistry) {
        self.slices = Some(register);
    }

    /// Syncs over `store` whatever else the app declares.
    pub(crate) fn store(&mut self, store: SyncStore) {
        self.store = Some(store);
    }

    /// Declares the projections `register` adds.
    pub(crate) fn projections(&mut self, register: fn(ProjectionRegistry) -> ProjectionRegistry) {
        self.projections = Some(register);
    }

    /// The sync server, resolved now so a check refuses an app with no store; connected when started.
    pub(crate) fn surface(
        &self,
        settings: &AppSettings,
        served: &Served,
    ) -> Result<Option<Deferred>, AnyError> {
        let Some(slices) = self.slices else {
            return Ok(None);
        };
        let server = Server {
            slices,
            projections: self.projections,
            auth: settings.config().auth.clone(),
            config: WsConfig::new(settings.sync_addr()?),
        };
        let store = Store::of(self.store, settings, served)?;
        let budget = WsServer::new(server.config.clone(), WsRouter::new()).drain_budget();
        Ok(Some(Deferred::new(
            SYNC,
            DrainPhase::Intake,
            Some(budget),
            move || Box::pin(store.start(server)),
        )))
    }
}

/// What the sync server is built from, beside its store.
struct Server {
    slices: fn(SliceRegistry) -> SliceRegistry,
    projections: Option<fn(ProjectionRegistry) -> ProjectionRegistry>,
    auth: AuthSettings,
    config: WsConfig,
}

impl Server {
    /// The served slices and the kept projections, registered.
    fn registries(&self) -> (SliceRegistry, ProjectionRegistry) {
        let projections = match self.projections {
            Some(register) => register(ProjectionRegistry::new()),
            None => ProjectionRegistry::new(),
        };
        ((self.slices)(SliceRegistry::new()), projections)
    }

    /// The server over `store`, its verifier loaded and its tables checked against it.
    async fn serve<T>(
        self,
        registry: SliceRegistry,
        projections: ProjectionRegistry,
        store: T,
    ) -> Result<LiveSyncSurface, AnyError>
    where
        T: SliceWriteStore + Send + Sync + 'static,
    {
        let verifier = Arc::new(JwtVerifier::from_settings(&self.auth).await?);
        LiveSyncServer::new(
            registry,
            projections,
            Arc::new(store),
            verifier,
            self.config,
        )
        .serve_with_capture(|_publisher, capture| refuse_capture(capture))
        .await
    }
}

/// Where the sync server reads and writes its slices.
enum Store {
    /// The app's default SQL pool.
    Sql(SqlPool),
    /// The app's default CQL keyspace, connected when the server starts.
    #[cfg(feature = "cql")]
    Cql(Box<CqlStore>),
}

impl Store {
    /// The store `chosen`, or when the app names none the one `settings` give: the default
    /// SQL pool when there is one.
    fn of(
        chosen: Option<SyncStore>,
        settings: &AppSettings,
        served: &Served,
    ) -> Result<Self, AnyError> {
        let database = &settings.config().database;
        let store = match chosen {
            Some(store) => store,
            #[cfg(feature = "cql")]
            None if database.sql.is_none() && database.cql.is_some() => SyncStore::Cql,
            None => SyncStore::Sql,
        };
        let declared = match store {
            SyncStore::Sql => chosen.is_none() || database.sql.is_some(),
            #[cfg(feature = "cql")]
            SyncStore::Cql => database.cql.is_some(),
        };
        if !declared {
            return Err(AnyError::new(AppError::SyncStoreUndeclared {
                app: settings.name().to_owned(),
                store: store.name(),
            }));
        }
        match store {
            SyncStore::Sql => Ok(Self::Sql(served.pool("default")?)),
            #[cfg(feature = "cql")]
            SyncStore::Cql => Ok(Self::Cql(Box::new(CqlStore {
                database: database.clone(),
                secrets: settings.config().secrets.clone(),
                environment: settings.environment(),
            }))),
        }
    }

    /// Starts `server` over this store.
    async fn start(self, server: Server) -> Result<Built, AnyError> {
        let (registry, projections) = server.registries();
        match self {
            // One store shared by the pull source and the projection writer.
            Self::Sql(pool) => Ok(Built::new(
                server
                    .serve(registry, projections, SqlSliceStore::new(&pool))
                    .await?,
            )),
            #[cfg(feature = "cql")]
            Self::Cql(cql) => {
                let CqlStore {
                    database,
                    secrets,
                    environment,
                } = *cql;
                let vault = fundamental_secrets::VaultClient::from_settings(&secrets).await?;
                let connections = fundamental_cql::CqlConnections::from_settings(
                    &database,
                    environment,
                    vault.as_ref(),
                )
                .await?;
                let pool = connections.pool_for_domain("default")?;
                let keyspace = database.cql.map(|cql| cql.keyspace).unwrap_or_default();
                // The CQL wire cannot introspect, so capture tables come from the mounted slices.
                let store =
                    fundamental_cql::CqlSliceStore::new(&pool, keyspace, registry.capture_tables());
                store.create_outbox_table().await?;
                let surface = server.serve(registry, projections, store).await?;
                Ok(Built::new(surface).holding(connections))
            }
        }
    }
}

/// What a CQL store is connected from when the sync server starts.
#[cfg(feature = "cql")]
struct CqlStore {
    database: fundamental_config::settings::DatabaseSettings,
    secrets: fundamental_config::settings::VaultSettings,
    environment: fundamental_core::Environment,
}

/// Refuses change capture: serving pull-only would break the guarantee a captured slice relies on.
async fn refuse_capture(capture: Vec<CaptureTable>) -> Result<Vec<Box<dyn Send>>, AnyError> {
    let tables = capture
        .iter()
        .map(CaptureTable::table)
        .collect::<Vec<_>>()
        .join(", ");
    Err(AnyError::new(SyncError::NoChangeCapture {
        count: capture.len(),
        tables,
    }))
}
