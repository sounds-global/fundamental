//! The [`CqlPool`] connection pool.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use fundamental_core::model::{BoundTo, CqlDb, CqlModel};
use fundamental_db::{DatabaseMetrics, DatabaseResult, PoolStatus};
use fundamental_encryption::SharedKeyManager;
use scylla::client::session::Session;
use scylla::deserialize::row::DeserializeRow;
use tokio::time::sleep;
use tracing::{debug, warn};

use crate::ident::validate_cql_identifier;
use crate::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerStats, CircuitState, CounterOps, Cql,
    CqlConfig, CqlExecutor, CqlQuery, CqlRepository, CqlResult, CqlSession, Ddl, FromRow,
    NoPartitionKey, OperationGuard, PreparedCache, ToRow,
};

/// YCQL connection pool.
#[derive(Clone)]
pub struct CqlPool {
    session: CqlSession,
    cache: Arc<PreparedCache>,
    config: Arc<CqlConfig>,
    circuit_breaker: Option<Arc<CircuitBreaker>>,
    leak_warnings: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
    in_flight: Arc<AtomicU64>,
    /// Handed to every repository this pool hands out, so `Secret` columns encrypt and decrypt.
    key_manager: Option<SharedKeyManager>,
    /// Where this pool's query timings are recorded.
    metrics: Option<Arc<dyn DatabaseMetrics>>,
}

/// The narrow "can it be reached" view a readiness probe needs.
impl fundamental_db::HealthCheck for CqlPool {
    async fn health_check(&self) -> DatabaseResult<()> {
        Self::health_check(self).await
    }
}

impl CqlPool {
    /// Creates a new connection pool.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn connect(config: &CqlConfig) -> CqlResult<Self> {
        let session = CqlSession::connect(config).await?;

        let cache = Arc::new(PreparedCache::new(
            session.clone(),
            config.prepared_cache_size,
        ));

        let circuit_breaker = config
            .circuit_breaker
            .as_ref()
            .map(|cb_config| Arc::new(CircuitBreaker::new(cb_config.clone())));

        let leak_warnings = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let in_flight = Arc::new(AtomicU64::new(0));

        debug!(
            hosts = ?config.contact_points,
            cache_size = config.prepared_cache_size,
            circuit_breaker = circuit_breaker.is_some(),
            retry = config.retry.is_some(),
            leak_detection = ?config.leak_detection_threshold,
            "YCQL pool created"
        );

        Ok(Self {
            session,
            cache,
            config: Arc::new(config.clone()),
            circuit_breaker,
            leak_warnings,
            shutdown,
            in_flight,
            key_manager: None,
            metrics: None,
        })
    }

    /// Enables circuit breaker with the given configuration.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fundamental_cql::{CircuitBreakerConfig, CqlConfig, CqlPool};
    ///
    /// # async fn example(config: CqlConfig) -> Result<(), Box<dyn std::error::Error>> {
    /// let pool = CqlPool::connect(&config).await?
    ///     .with_circuit_breaker(CircuitBreakerConfig::default());
    /// # let _ = pool;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_circuit_breaker(mut self, config: CircuitBreakerConfig) -> Self {
        self.circuit_breaker = Some(Arc::new(CircuitBreaker::new(config)));
        debug!("Circuit breaker enabled for YCQL pool");
        self
    }

    /// Returns the circuit breaker if enabled.
    pub fn circuit_breaker(&self) -> Option<&Arc<CircuitBreaker>> {
        self.circuit_breaker.as_ref()
    }

    /// Returns the current circuit breaker state, or Closed if not enabled.
    pub fn circuit_state(&self) -> CircuitState {
        self.circuit_breaker
            .as_ref()
            .map(|cb| cb.state())
            .unwrap_or(CircuitState::Closed)
    }

    /// Returns circuit breaker statistics, or None if not enabled.
    pub fn circuit_stats(&self) -> Option<CircuitBreakerStats> {
        self.circuit_breaker.as_ref().map(|cb| cb.stats())
    }

    /// Checks if requests are allowed (circuit is not open).
    pub fn is_circuit_allowed(&self) -> bool {
        self.circuit_breaker
            .as_ref()
            .map(|cb| cb.is_allowed())
            .unwrap_or(true)
    }

    /// Returns a repository for the given model.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fundamental_cql::{CqlValue, CqlPool};
    /// # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
    /// # #[model(cql, table = "events")]
    /// # struct Event {
    /// #     #[model(partition_key)]
    /// #     tenant_id: uuid::Uuid,
    /// #     payload: String,
    /// # }
    /// # async fn example(pool: CqlPool, user_id: uuid::Uuid) -> Result<(), Box<dyn std::error::Error>> {
    /// let events = pool.repository::<Event>();
    /// events
    ///     .query()
    ///     .partition_key(CqlValue::uuid(user_id))
    ///     .execute()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn repository<M>(&self) -> CqlRepository<M>
    where
        M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
    {
        let repo = CqlRepository::new(
            self.session.clone(),
            Arc::clone(&self.cache),
            self.config.default_keyspace.clone().unwrap_or_default(),
            self.config.read_consistency,
            self.config.write_consistency,
        );
        let repo = match &self.key_manager {
            Some(km) => repo.with_key_manager(km.clone()),
            None => repo,
        };
        match &self.metrics {
            Some(metrics) => repo.with_metrics(Arc::clone(metrics), SLOW_QUERY_THRESHOLD),
            None => repo,
        }
    }

    /// Attaches a key manager, so every repository this pool hands out transparently
    /// encrypts/decrypts `Secret` model columns.
    #[must_use]
    pub fn with_key_manager(mut self, key_manager: SharedKeyManager) -> Self {
        self.key_manager = Some(key_manager);
        self
    }

    /// Returns the attached key manager, if any.
    #[must_use]
    pub fn key_manager(&self) -> Option<&SharedKeyManager> {
        self.key_manager.as_ref()
    }

    /// Attaches the collector every repository this pool hands out records its
    /// query timings to.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn DatabaseMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Returns the attached metrics collector, if any.
    #[must_use]
    pub fn metrics(&self) -> Option<&Arc<dyn DatabaseMetrics>> {
        self.metrics.as_ref()
    }

    /// Returns a query builder for the given model.
    ///
    /// This is a convenience method equivalent to `pool.repository::<M>().query()`.
    pub fn query<M>(&self) -> CqlQuery<M, NoPartitionKey>
    where
        M: BoundTo<CqlDb> + CqlModel,
    {
        CqlQuery::new(
            self.session.clone(),
            Arc::clone(&self.cache),
            self.config.default_keyspace.clone().unwrap_or_default(),
            // Carries the pool's key manager (if any), so a query built off a
            // registry-configured pool reads `Secret` columns.
            self.key_manager.clone(),
        )
    }

    /// Returns an executor for raw CQL against this pool's keyspace.
    ///
    /// For schema and administration; model reads and writes belong on
    /// [`repository`](Self::repository). It carries the pool's consistency and metrics.
    #[must_use]
    pub fn cql(&self) -> CqlExecutor {
        self.cql_for(self.config.default_keyspace.clone().unwrap_or_default())
    }

    /// Returns an executor for raw CQL against a named keyspace.
    #[must_use]
    pub fn cql_for(&self, keyspace: impl Into<String>) -> CqlExecutor {
        let executor = CqlExecutor::new(
            self.session.clone(),
            Arc::clone(&self.cache),
            keyspace,
            self.config.read_consistency,
            self.config.write_consistency,
        )
        .with_ddl_timeout(self.config.ddl_timeout);
        match &self.metrics {
            Some(metrics) => executor.with_metrics(Arc::clone(metrics), SLOW_QUERY_THRESHOLD),
            None => executor,
        }
    }

    /// Runs a schema statement.
    ///
    /// # Errors
    ///
    /// [`CqlError::Query`](crate::CqlError::Query) if the statement fails.
    pub async fn execute_ddl(&self, statement: &Ddl) -> CqlResult<()> {
        let _guard = self.operation_guard_named("execute_ddl");
        self.cql().execute_ddl(statement).await
    }

    /// Runs a data statement that returns no rows.
    ///
    /// # Errors
    ///
    /// [`CqlError::Prepared`](crate::CqlError::Prepared) if the statement cannot be prepared, or
    /// [`CqlError::Query`](crate::CqlError::Query) if it fails.
    pub async fn execute(&self, statement: &Cql) -> CqlResult<()> {
        let _guard = self.operation_guard_named("execute");
        self.cql().execute(statement).await
    }

    /// Runs a conditional write and reports whether its condition held (`[applied]`).
    ///
    /// # Errors
    ///
    /// See [`CqlExecutor::execute_conditional`].
    pub async fn execute_conditional(&self, statement: &Cql) -> CqlResult<bool> {
        let _guard = self.operation_guard_named("execute_conditional");
        self.cql().execute_conditional(statement).await
    }

    /// Reads rows, refusing a result set larger than
    /// [`DEFAULT_MAX_UNPAGED_RESULTS`](crate::DEFAULT_MAX_UNPAGED_RESULTS).
    ///
    /// # Errors
    ///
    /// See [`CqlExecutor::fetch_all`].
    pub async fn fetch_all<R>(&self, statement: &Cql) -> CqlResult<Vec<R>>
    where
        R: for<'frame, 'metadata> DeserializeRow<'frame, 'metadata>,
    {
        let _guard = self.operation_guard_named("fetch_all");
        self.cql().fetch_all(statement).await
    }

    /// Reads up to `max_rows` rows, a page at a time.
    ///
    /// # Errors
    ///
    /// See [`CqlExecutor::fetch_paged`].
    pub async fn fetch_paged<R>(&self, statement: &Cql, max_rows: usize) -> CqlResult<Vec<R>>
    where
        R: for<'frame, 'metadata> DeserializeRow<'frame, 'metadata>,
    {
        let _guard = self.operation_guard_named("fetch_paged");
        self.cql().fetch_paged(statement, max_rows).await
    }

    /// Reads exactly one row.
    ///
    /// # Errors
    ///
    /// See [`CqlExecutor::fetch_one`].
    pub async fn fetch_one<R>(&self, statement: &Cql) -> CqlResult<R>
    where
        R: for<'frame, 'metadata> DeserializeRow<'frame, 'metadata>,
    {
        let _guard = self.operation_guard_named("fetch_one");
        self.cql().fetch_one(statement).await
    }

    /// Reads at most one row.
    ///
    /// # Errors
    ///
    /// See [`CqlExecutor::fetch_optional`].
    pub async fn fetch_optional<R>(&self, statement: &Cql) -> CqlResult<Option<R>>
    where
        R: for<'frame, 'metadata> DeserializeRow<'frame, 'metadata>,
    {
        let _guard = self.operation_guard_named("fetch_optional");
        self.cql().fetch_optional(statement).await
    }

    /// Returns counter operations for `table` in this pool's keyspace.
    ///
    /// Inherits the pool's keyspace and metrics collector.
    ///
    /// # Errors
    ///
    /// [`CqlError::InvalidIdentifier`](crate::CqlError::InvalidIdentifier) if `table` is not a plain identifier.
    pub fn counters(&self, table: &str) -> CqlResult<CounterOps> {
        validate_cql_identifier(table, false)?;
        let counters = CounterOps::new(
            self.session.clone(),
            Arc::clone(&self.cache),
            self.config.default_keyspace.clone().unwrap_or_default(),
            table,
        );
        Ok(match &self.metrics {
            Some(metrics) => counters.with_metrics(Arc::clone(metrics), SLOW_QUERY_THRESHOLD),
            None => counters,
        })
    }

    /// Returns the underlying session.
    ///
    /// Prefer [`cql`](Self::cql), which prepares, pages and records timings.
    pub fn session(&self) -> &CqlSession {
        &self.session
    }

    /// Moves every handle on this pool onto `fresh`'s session, returning the one it replaced.
    ///
    /// Prepared statements are prepared again on the new session at their next use.
    pub async fn rotate_to(&self, fresh: &CqlPool) -> Arc<Session> {
        let replaced = self.session.rotate_to(&fresh.session);
        self.cache.clear().await;
        replaced
    }

    /// Returns the prepared statement cache.
    pub fn cache(&self) -> &Arc<PreparedCache> {
        &self.cache
    }

    /// Returns the configuration.
    pub fn config(&self) -> &CqlConfig {
        &self.config
    }

    /// Checks if the pool is healthy.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn health_check(&self) -> DatabaseResult<()> {
        self.session.health_check().await.map_err(Into::into)
    }

    /// Returns pool status.
    ///
    /// `active` is measured from the driver; `idle`, `min` and `pending` are `None`, as the driver
    /// has no such counts.
    pub fn status(&self) -> PoolStatus {
        let cluster_info = self.session.cluster_info();
        let node_count = u32::try_from(cluster_info.nodes.len()).unwrap_or(u32::MAX);
        let connections_per_node = u32::try_from(self.config.connections_per_host).unwrap_or(1);

        PoolStatus {
            active: None,
            idle: None,
            max: node_count.saturating_mul(connections_per_node),
            min: None,
            pending: None,
        }
    }

    /// Closes the pool.
    ///
    /// The driver session has no close; connections close when it is dropped.
    pub async fn close(&self) {
        debug!("Closing YCQL pool");
    }

    /// Performs a graceful shutdown: refuses new operations and waits up to `timeout` for in-flight
    /// ones, returning `Err` with how many remain.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use std::time::Duration;
    /// use fundamental_cql::{CqlConfig, CqlPool};
    ///
    /// # async fn example(config: CqlConfig) -> Result<(), Box<dyn std::error::Error>> {
    /// let pool = CqlPool::connect(&config).await?;
    ///
    /// // On shutdown signal
    /// match pool.graceful_shutdown(Duration::from_secs(30)).await {
    ///     Ok(()) => println!("Clean shutdown"),
    ///     Err(remaining) => println!("Forced shutdown with {} pending ops", remaining),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub async fn graceful_shutdown(&self, timeout: Duration) -> Result<(), u64> {
        debug!("Initiating graceful shutdown");

        self.shutdown.store(true, Ordering::SeqCst);

        let start = Instant::now();
        let poll_interval = Duration::from_millis(50);

        loop {
            let remaining = self.in_flight.load(Ordering::Relaxed);

            if remaining == 0 {
                debug!("Graceful shutdown complete - all operations finished");
                return Ok(());
            }

            if start.elapsed() >= timeout {
                warn!(
                    remaining = remaining,
                    timeout_ms = timeout.as_millis(),
                    "Graceful shutdown timeout - forcing close"
                );
                return Err(remaining);
            }

            debug!(
                remaining = remaining,
                elapsed_ms = start.elapsed().as_millis(),
                "Waiting for in-flight operations to complete"
            );

            sleep(poll_interval).await;
        }
    }

    /// Returns whether the pool is shutting down.
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }

    /// Returns the number of in-flight operations.
    pub fn in_flight_count(&self) -> u64 {
        self.in_flight.load(Ordering::Relaxed)
    }

    /// Returns prepared statement cache statistics.
    pub async fn cache_stats(&self) -> crate::prepared::CacheStats {
        self.cache.stats().await
    }

    /// Clears the prepared statement cache.
    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    /// Creates an operation guard for leak detection and graceful shutdown tracking.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fundamental_db::Repository;
    /// use fundamental_cql::{CqlConfig, CqlPool};
    /// # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
    /// # #[model(cql, table = "events")]
    /// # struct Event {
    /// #     #[model(partition_key)]
    /// #     tenant_id: uuid::Uuid,
    /// #     payload: String,
    /// # }
    /// # async fn example(config: CqlConfig, event: Event) -> Result<(), Box<dyn std::error::Error>> {
    /// let pool = CqlPool::connect(&config).await?;
    /// let guard = pool.operation_guard();
    ///
    /// // Do work with session
    /// pool.repository::<Event>().insert(&event).await?;
    ///
    /// // Guard is dropped here, checking operation duration
    /// drop(guard);
    /// # Ok(())
    /// # }
    /// ```
    pub fn operation_guard(&self) -> OperationGuard {
        OperationGuard::with_in_flight(
            self.config.leak_detection_threshold,
            Arc::clone(&self.leak_warnings),
            Arc::clone(&self.in_flight),
        )
    }

    /// Creates an operation guard with an operation name for better logging.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fundamental_cql::{BatchType, CqlBatch, CqlConfig, CqlPool};
    /// # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
    /// # #[model(cql, table = "events")]
    /// # struct Event {
    /// #     #[model(partition_key)]
    /// #     tenant_id: uuid::Uuid,
    /// #     #[model(clustering_key)]
    /// #     bucket: i64,
    /// #     payload: String,
    /// # }
    /// # async fn example(config: CqlConfig, batches: Vec<Vec<Event>>) -> Result<(), Box<dyn std::error::Error>> {
    /// let pool = CqlPool::connect(&config).await?;
    /// let guard = pool.operation_guard_named("batch_insert");
    ///
    /// for batch in batches {
    ///     pool.repository::<Event>().insert_batch(&batch, BatchType::Unlogged).await?;
    /// }
    ///
    /// // Warning logged if this block takes > threshold
    /// drop(guard);
    /// # Ok(())
    /// # }
    /// ```
    pub fn operation_guard_named(&self, operation: impl Into<String>) -> OperationGuard {
        OperationGuard::with_in_flight_named(
            self.config.leak_detection_threshold,
            Arc::clone(&self.leak_warnings),
            Arc::clone(&self.in_flight),
            operation,
        )
    }

    /// Returns the number of leak warnings recorded.
    pub fn leak_warning_count(&self) -> u64 {
        self.leak_warnings.load(Ordering::Relaxed)
    }
}

/// The elapsed time above which a query is additionally recorded as slow.
///
/// `fundamental_db::MetricsConfig`'s default, as `CqlConfig` carries no threshold.
const SLOW_QUERY_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(1);

impl std::fmt::Debug for CqlPool {
    /// The collector is not `Debug`; whether one is attached is what is printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CqlPool")
            .field("config", &self.config)
            .field("has_circuit_breaker", &self.circuit_breaker.is_some())
            .field("has_key_manager", &self.key_manager.is_some())
            .field("has_metrics", &self.metrics.is_some())
            .field("in_flight", &self.in_flight.load(Ordering::Relaxed))
            .finish()
    }
}
