use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerStats, CircuitState, ConnectionGuard,
    FromRow, SqlConfig, SqlError, SqlQuery, SqlQueryExecutor, SqlRepository, SqlResult,
    SqlTransaction, ToRow, commit_outcome_unknown,
    error::is_pool_exhaustion,
    outbox::{CLOCK_SKEW, ClaimedRow},
    sqlstate_of,
};

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::AnyError;
use fundamental_core::Error as _;
use fundamental_core::model::{BoundTo, Model, SqlDb, SqlModel};
use fundamental_db::{
    ConnectionPool, DatabaseError, DatabaseMetrics, DatabaseResult, Filter, HeldCount,
    IsolationLevel, OrderBy, OutboxClaim, OutboxClaimer, OutboxColumns, OutboxInventory, OutboxRow,
    OutboxSetAside, PoolStatus, RefusalKind, RepositoryTransactionBatch, RetryPolicy, SchemaName,
    SetAsideBucket, TransactionOptions, Transactional, finalize_observed,
};
use fundamental_db::{DDL_TIME_BUDGET, TlsMode};
use fundamental_encryption::{KeyManager, SharedKeyManager};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};

use super::{CommitAnswer, RotatedPools, SqlConnector, SqlReservation};
use tracing::field::Empty;
use tracing::{Span, debug, error, info, instrument, warn};

/// The floor on DDL attempts: DDL contends on a cluster-wide catalog latch, so the query budget is too small.
const DDL_ATTEMPTS: u32 = 24;

/// YSQL connection pool, retrying serialization failures.
#[derive(Clone)]
pub struct SqlPool {
    connector: SqlConnector,
    config: Arc<SqlConfig>,
    circuit_breaker: Option<Arc<CircuitBreaker>>,
    /// `dyn`, so a pool feeds either an in-process [`SqlMetrics`] or a Prometheus scrape.
    metrics: Option<Arc<dyn DatabaseMetrics>>,
    leak_warnings: Arc<AtomicU64>,
    /// Inherited by every repository and transaction this pool hands out, so `Secret` columns encrypt on every path.
    key_manager: Option<SharedKeyManager>,
    /// The schema every statement this pool builds qualifies its tables with.
    schema: SchemaName,
}

impl std::fmt::Debug for SqlPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlPool")
            .field("connector", &self.connector)
            .field("config", &self.config)
            .field("has_circuit_breaker", &self.circuit_breaker.is_some())
            .field("has_metrics", &self.metrics.is_some())
            .field("leak_warnings", &self.leak_warnings.load(Ordering::Relaxed))
            .field("has_key_manager", &self.key_manager.is_some())
            .finish()
    }
}

impl SqlPool {
    /// Creates a new connection pool.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn connect(config: &SqlConfig) -> SqlResult<Self> {
        let pool = pool_options(config)
            .connect_with(connect_options(&config.pool)?)
            .await?;

        debug!(
            url = %config.pool.url,
            min = config.pool.min_connections,
            max = config.pool.max_connections,
            statement_timeout = ?config.pool.statement_timeout,
            leak_detection = ?config.leak_detection_threshold,
            "Connected to YugabyteDB (YSQL)"
        );

        let config_held = Arc::new(config.clone());
        Ok(Self {
            connector: SqlConnector::new(pool).with_config(Arc::clone(&config_held)),
            config: config_held,
            circuit_breaker: None,
            metrics: None,
            leak_warnings: Arc::new(AtomicU64::new(0)),
            key_manager: None,
            schema: config.default_schema.clone(),
        })
    }

    /// A pool over `config` that opens no connection until a statement needs one.
    ///
    /// # Errors
    ///
    /// [`SqlError`] when `config`'s connection options do not parse.
    pub fn connect_lazy(config: &SqlConfig) -> SqlResult<Self> {
        let pool = pool_options(config).connect_lazy_with(connect_options(&config.pool)?);
        let config_held = Arc::new(config.clone());
        Ok(Self {
            connector: SqlConnector::new(pool).with_config(Arc::clone(&config_held)),
            config: config_held,
            circuit_breaker: None,
            metrics: None,
            leak_warnings: Arc::new(AtomicU64::new(0)),
            key_manager: None,
            schema: config.default_schema.clone(),
        })
    }

    /// Opens `reservation`'s connections apart from this pool's, for work that must not wait
    /// behind the shared pool; registered here, so [`rotate_to`](Self::rotate_to) moves it too.
    ///
    /// # Errors
    ///
    /// [`SqlError`] when the reserved connections cannot be opened.
    #[instrument(name = "sql.reserve", skip(self), fields(name = reservation.name(), connections = reservation.connections()))]
    pub async fn reserve(&self, reservation: &SqlReservation) -> SqlResult<SqlPool> {
        let metrics = self.metrics.clone();
        let (connector, config) =
            self.connector
                .reserve_with(&self.config, reservation, |current| {
                    let config = reserved_config(current, reservation);
                    let opened =
                        pool_options(&config).connect_lazy_with(connect_options(&config.pool)?);
                    let mut connector = SqlConnector::new(opened).with_label(reservation.name());
                    if let Some(metrics) = &metrics {
                        connector = connector.with_metrics(Arc::clone(metrics));
                    }
                    Ok((connector, config))
                })?;
        // One acquire here refuses a credential or database that cannot serve now, not the first statement.
        if let Err(error) = connector.acquire().await {
            warn!(error = %error, "reserved connections could not be opened");
            return Err(error);
        }
        info!("reserved connections opened");
        Ok(Self {
            connector,
            config: Arc::new(config),
            circuit_breaker: None,
            metrics: self.metrics.clone(),
            leak_warnings: Arc::new(AtomicU64::new(0)),
            key_manager: self.key_manager.clone(),
            schema: self.schema.clone(),
        })
    }

    /// Creates a new connection pool, retrying the first connection with exponential backoff.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use fundamental_sql::{SqlPool, SqlConfig};
    /// use std::time::Duration;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = SqlConfig::new("postgresql://localhost:5433/db");
    /// let pool = SqlPool::connect_with_retry(&config, 5, Duration::from_secs(1)).await?;
    /// # let _ = pool;
    /// # Ok(())
    /// # }
    /// ```
    #[expect(
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub async fn connect_with_retry(
        config: &SqlConfig,
        max_retries: u32,
        base_delay: Duration,
    ) -> SqlResult<Self> {
        let mut attempts = 0u32;
        let max_delay = Duration::from_secs(30);

        loop {
            match Self::connect(config).await {
                Ok(pool) => {
                    if attempts > 0 {
                        debug!(
                            attempts = attempts,
                            "Connected to YugabyteDB (YSQL) after retry"
                        );
                    }
                    return Ok(pool);
                }
                Err(err) => {
                    attempts += 1;

                    if attempts > max_retries {
                        warn!(
                            attempts = attempts,
                            error = %err,
                            "Failed to connect to YugabyteDB (YSQL) after max retries"
                        );
                        return Err(err);
                    }

                    let delay_ms = base_delay.as_millis() as u64 * (1u64 << (attempts - 1).min(10));
                    let delay = Duration::from_millis(delay_ms.min(max_delay.as_millis() as u64));

                    warn!(
                        attempt = attempts,
                        max_retries = max_retries,
                        delay_ms = delay.as_millis(),
                        error = %err,
                        "Failed to connect to YugabyteDB (YSQL), retrying"
                    );

                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    /// Enables a circuit breaker with the given configuration.
    pub fn with_circuit_breaker(mut self, config: CircuitBreakerConfig) -> Self {
        self.circuit_breaker = Some(Arc::new(CircuitBreaker::new(config)));
        debug!("Circuit breaker enabled for YSQL pool");
        self
    }

    /// Sets the metrics collector every repository, query executor and transaction this pool hands out records through.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn DatabaseMetrics>) -> Self {
        // Both: every acquire goes through the connector, so the acquire metrics need it there.
        self.connector = self.connector.with_metrics(Arc::clone(&metrics));
        self.metrics = Some(metrics);
        debug!("Metrics collector attached to YSQL pool");
        self
    }

    /// The schema this pool's statements qualify their tables with.
    #[must_use]
    pub fn schema(&self) -> &SchemaName {
        &self.schema
    }

    /// The attached metrics collector, if any.
    #[must_use]
    pub fn metrics(&self) -> Option<&Arc<dyn DatabaseMetrics>> {
        self.metrics.as_ref()
    }

    fn record_retry(&self, sqlstate: &str) {
        if let Some(metrics) = &self.metrics {
            metrics.record_retry(sqlstate);
        }
    }

    fn record_retry_exhausted(&self, sqlstate: &str) {
        if let Some(metrics) = &self.metrics {
            metrics.record_retry_exhausted(sqlstate);
        }
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

    /// Returns the number of leak warnings recorded.
    pub fn leak_warning_count(&self) -> u64 {
        self.leak_warnings.load(Ordering::Relaxed)
    }

    /// Records a connection leak warning.
    #[allow(dead_code)]
    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    fn record_leak_warning(&self) {
        self.leak_warnings.fetch_add(1, Ordering::Relaxed);
    }

    /// The sqlx pool serving now; an unobserved acquire, so use [`connector`](Self::connector) for statements, and never keep it.
    #[must_use]
    pub fn inner(&self) -> PgPool {
        self.connector.pool()
    }

    /// Serves every handle cloned from this pool, and every reservation, from `fresh`'s
    /// connections from now on, returning the replaced ones for the caller to drain and close.
    pub fn rotate_to(&self, fresh: &SqlPool) -> RotatedPools {
        let primary = self.connector.replace_pool(fresh.connector.pool());
        let (reserved, failed) = self.connector.replace_reservations(
            Arc::clone(&fresh.config),
            |current, reservation| {
                let config = reserved_config(current, reservation);
                Ok(pool_options(&config).connect_lazy_with(connect_options(&config.pool)?))
            },
        );
        for (reservation, error) in failed {
            error!(
                reservation = reservation.name(),
                error = %error,
                "a reservation could not follow the rotated credential"
            );
        }
        RotatedPools::new(primary, reserved)
    }

    /// The seam every statement this pool serves goes through, so its acquisitions are counted.
    #[must_use]
    pub fn connector(&self) -> &SqlConnector {
        &self.connector
    }

    /// Returns the configuration.
    pub fn config(&self) -> &SqlConfig {
        &self.config
    }

    /// Executes a function, running it again while [`SqlError::is_retryable`] says so, up to [`SqlConfig::max_retries`].
    ///
    /// # Errors
    ///
    /// The last error once the budget is spent, or the first non-retryable one.
    pub async fn with_retry<F, Fut, T>(&self, f: F) -> SqlResult<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = SqlResult<T>>,
    {
        self.with_retry_budget(self.config.max_retries + 1, f).await
    }

    /// [`with_retry`](Self::with_retry) with the number of attempts stated rather than inherited.
    ///
    /// # Errors
    ///
    /// The last error once `attempts` is spent, or the first non-retryable one. An `attempts`
    /// of zero is treated as one: the function is always run.
    pub async fn with_retry_budget<F, Fut, T>(&self, attempts: u32, f: F) -> SqlResult<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = SqlResult<T>>,
    {
        self.with_retry_within(attempts, Duration::MAX, f).await
    }

    /// [`with_retry_budget`](Self::with_retry_budget) bounded in time as well, checked between attempts.
    ///
    /// # Errors
    ///
    /// The last error once either bound is reached, or the first non-retryable one.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub async fn with_retry_within<F, Fut, T>(
        &self,
        attempts: u32,
        within: Duration,
        f: F,
    ) -> SqlResult<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = SqlResult<T>>,
    {
        let deadline = Instant::now().checked_add(within);
        for attempt in 1..attempts.max(1) {
            if deadline.is_some_and(|end| Instant::now() >= end) {
                break;
            }
            match f().await {
                Ok(result) => return Ok(result),
                Err(error) if error.should_retry_in_process() => {
                    let sqlstate = error.sqlstate().unwrap_or("unknown");
                    self.record_retry(sqlstate);
                    let delay = self.calculate_retry_delay(attempt);
                    warn!(
                        attempt,
                        attempts,
                        delay_ms = delay.as_millis(),
                        sqlstate,
                        "retryable failure, running it again"
                    );
                    tokio::time::sleep(delay).await;
                }
                Err(error) => return Err(error),
            }
        }

        let outcome = f().await;
        if let Err(error) = &outcome
            && error.should_retry_in_process()
        {
            let sqlstate = error.sqlstate().unwrap_or("unknown");
            self.record_retry_exhausted(sqlstate);
            warn!(
                attempts = attempts.max(1),
                sqlstate,
                code = error.error_code(),
                "retry budget spent; the failure reaches the caller"
            );
        }
        outcome
    }

    /// Runs a transaction, running it again whole while the engine says the conflict is retryable.
    ///
    /// The closure is `Clone` as proof that it may be run again. Effects outside the database
    /// must be built before it and committed after it. A commit that lost its answer is never
    /// run again.
    ///
    /// # Errors
    ///
    /// The closure's error once the retry budget is spent, or a [`DatabaseError`] if
    /// beginning, committing or rolling back fails —
    /// [`CommitOutcomeUnknown`](DatabaseError::CommitOutcomeUnknown) when a commit's answer
    /// was lost.
    #[instrument(level = "debug", skip(self, f), fields(op = "transaction", isolation = %level.as_sql()))]
    pub async fn transaction<F, T, E>(&self, level: IsolationLevel, f: F) -> Result<T, E>
    where
        F: AsyncFnOnce(&mut SqlTransaction) -> Result<T, E> + Clone,
        E: From<DatabaseError> + fundamental_core::Error,
    {
        for attempt in 1..=self.config.max_retries {
            let (outcome, conflict, answer) = self.run_once(level, f.clone()).await;
            let error = match outcome {
                Ok(value) => {
                    if attempt > 1 {
                        info!(attempt, "transaction committed after a retry");
                    }
                    return Ok(value);
                }
                Err(error) => error,
            };
            if answer == CommitAnswer::Lost {
                error!(
                    attempt,
                    code = error.error_code(),
                    "commit outcome unknown; the transaction is not run again"
                );
                return Err(error);
            }
            // The engine's verdict outranks `E`, which a closure may have flattened into a string.
            if conflict.is_none() && (!error.is_retryable() || is_pool_exhaustion(&error)) {
                return Err(error);
            }
            let sqlstate = conflict.unwrap_or("unknown");
            self.record_retry(sqlstate);
            let delay = self.calculate_retry_delay(attempt);
            warn!(
                attempt,
                max_retries = self.config.max_retries,
                delay_ms = delay.as_millis(),
                sqlstate,
                code = error.error_code(),
                "retryable transaction conflict; running it again"
            );
            tokio::time::sleep(delay).await;
        }

        let (outcome, conflict, answer) = self.run_once(level, f).await;
        if let Err(error) = &outcome
            && answer == CommitAnswer::Lost
        {
            error!(
                attempt = self.config.max_retries + 1,
                code = error.error_code(),
                "commit outcome unknown; the transaction is not run again"
            );
        } else if let Err(error) = &outcome
            && (conflict.is_some() || error.is_retryable())
            && !is_pool_exhaustion(error)
        {
            let sqlstate = conflict.unwrap_or("unknown");
            self.record_retry_exhausted(sqlstate);
            warn!(
                attempts = self.config.max_retries + 1,
                sqlstate,
                code = error.error_code(),
                "retry budget spent; the conflict reaches the caller"
            );
        }
        outcome
    }

    /// One begin/run/commit-or-rollback cycle, with the SQLSTATE recorded on the transaction before `E` can flatten it.
    async fn run_once<F, T, E>(
        &self,
        level: IsolationLevel,
        f: F,
    ) -> (Result<T, E>, Option<&'static str>, CommitAnswer)
    where
        F: AsyncFnOnce(&mut SqlTransaction) -> Result<T, E>,
        E: From<DatabaseError>,
    {
        let mut tx = match self.begin(level).await {
            Ok(tx) => tx,
            Err(error) => {
                let sqlstate = sqlstate_of(&error);
                return (Err(E::from(error)), sqlstate, CommitAnswer::Given);
            }
        };
        let outcome = f(&mut tx).await;
        // Read before `finalize` consumes the transaction and before any `From` on the way out.
        let mut conflict = tx.conflict();
        // The commit has no transaction left to ask, so its refusal class is read off the error here.
        let mut answer = CommitAnswer::Given;
        let outcome = finalize_observed(tx, outcome, |error| {
            conflict = conflict.or_else(|| sqlstate_of(&error));
            let error = commit_outcome_unknown(error);
            if matches!(error, DatabaseError::CommitOutcomeUnknown { .. }) {
                answer = CommitAnswer::Lost;
            }
            error
        })
        .await;
        (outcome, conflict, answer)
    }

    fn calculate_retry_delay(&self, attempt: u32) -> Duration {
        calculate_retry_delay_internal(
            self.config.retry_base_delay,
            self.config.retry_max_delay,
            attempt,
        )
    }

    /// Executes a raw SQL query.
    #[instrument(level = "debug", skip(self, sql), fields(op = "pool_execute", rows_affected = Empty), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute(&self, sql: &str) -> SqlResult<u64> {
        let result = sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut *self.connector.acquire().await?)
            .await?;
        let affected = result.rows_affected();
        Span::current().record("rows_affected", affected);
        Ok(affected)
    }

    /// Executes one idempotent DDL statement, retrying the catalog contention this engine reports
    /// as a serialization conflict, at least `DDL_ATTEMPTS` times.
    ///
    /// # Errors
    /// Returns the underlying [`SqlError`](crate::SqlError) once the retry budget is spent,
    /// or immediately for anything the engine does not call retryable.
    #[instrument(
        level = "debug",
        skip(self, sql),
        fields(op = "pool_execute_ddl", attempts = Empty),
        err(Debug)
    )]
    pub async fn execute_ddl(&self, sql: &str) -> SqlResult<u64> {
        let attempts = ddl_attempts(self.config.max_retries + 1);
        Span::current().record("attempts", attempts);
        self.with_retry_within(attempts, DDL_TIME_BUDGET, || async {
            self.execute(sql).await
        })
        .await
    }

    /// Executes a raw SQL query and returns rows.
    #[instrument(level = "debug", skip(self, sql), fields(op = "pool_fetch_all", rows = Empty), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn fetch_all(&self, sql: &str) -> SqlResult<Vec<sqlx::postgres::PgRow>> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut *self.connector.acquire().await?)
            .await?;
        Span::current().record("rows", rows.len());
        Ok(rows)
    }

    /// Executes a raw SQL query and returns a single row.
    #[instrument(
        level = "debug",
        skip(self, sql),
        fields(op = "pool_fetch_one"),
        err(Debug)
    )]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn fetch_one(&self, sql: &str) -> SqlResult<sqlx::postgres::PgRow> {
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_one(&mut *self.connector.acquire().await?)
            .await?;
        Ok(row)
    }

    /// Executes a raw SQL query and returns an optional row.
    #[instrument(level = "debug", skip(self, sql), fields(op = "pool_fetch_optional", found = Empty), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn fetch_optional(&self, sql: &str) -> SqlResult<Option<sqlx::postgres::PgRow>> {
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_optional(&mut *self.connector.acquire().await?)
            .await?;
        Span::current().record("found", row.is_some());
        Ok(row)
    }
}

impl SqlPool {
    /// Creates a connection guard that warns when a connection is held past the configured threshold.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # async fn example(config: &fundamental_sql::SqlConfig) -> Result<(), Box<dyn std::error::Error>> {
    /// # use fundamental_sql::SqlPool;
    /// let pool = SqlPool::connect(config).await?;
    /// let guard = pool.connection_guard();
    ///
    /// // Do work with connection
    /// pool.execute("SELECT 1").await?;
    ///
    /// // Guard is dropped here, checking hold time
    /// drop(guard);
    /// # Ok(())
    /// # }
    /// ```
    pub fn connection_guard(&self) -> ConnectionGuard {
        ConnectionGuard::new(
            self.config.leak_detection_threshold,
            Arc::clone(&self.leak_warnings),
        )
    }
}

impl fundamental_db::HealthCheck for SqlPool {
    async fn health_check(&self) -> DatabaseResult<()> {
        ConnectionPool::health_check(self).await
    }
}

impl ConnectionPool<SqlDb> for SqlPool {
    type Config = SqlConfig;

    async fn connect(config: &Self::Config) -> DatabaseResult<Self> {
        Self::connect(config).await.map_err(Into::into)
    }

    fn status(&self) -> PoolStatus {
        let pool = self.connector.pool();
        let size = pool.size();
        let idle = u32::try_from(pool.num_idle()).unwrap_or(u32::MAX);

        PoolStatus {
            active: Some(size.saturating_sub(idle)),
            idle: Some(idle),
            max: pool.options().get_max_connections(),
            min: Some(pool.options().get_min_connections()),
            // sqlx exposes no waiter count; only the code doing the awaiting can count one.
            pending: Some(self.connector.waiting()),
        }
    }

    async fn close(&self) {
        self.connector.pool().close().await;
    }

    /// Whether traffic should still be routed here: a saturated pool answers yes, since it is load,
    /// not an outage; only a pool holding no established connection answers no.
    #[instrument(level = "debug", skip(self), fields(op = "health_check"), err(Debug))]
    async fn health_check(&self) -> DatabaseResult<()> {
        let mut connection = match self.connector.acquire().await {
            Ok(connection) => connection,
            Err(error) if error.is_pool_exhaustion() => {
                let census = self.status();
                let established = census.active.unwrap_or(0) + census.idle.unwrap_or(0);
                if established == 0 {
                    return Err(error.into());
                }
                warn!(
                    established,
                    waiting = self.connector.waiting(),
                    max = census.max,
                    "ready and saturated: every connection is busy, and the store is answering"
                );
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        sqlx::query("SELECT 1")
            .execute(&mut *connection)
            .await
            .map(|_| ())
            .map_err(|e| SqlError::from(e).into())
    }

    /// A no-op: SQLx cannot resize a pool after it is created.
    ///
    /// ```rust,no_run
    /// # use fundamental_db::ConnectionPool;
    /// # fn example(pool: &fundamental_sql::SqlPool) {
    /// let status = pool.status();
    /// // `None` where the backend cannot count what it holds, so an unmeasured pool
    /// // reads as an absent series rather than as a comfortable zero.
    /// if let Some(used) = status.utilization()
    ///     && used >= 90.0
    /// {
    ///     println!("Pool near exhaustion: {used:.0}% of {}", status.max);
    /// }
    /// # }
    /// ```
    async fn resize(&self, min: u32, max: u32) -> DatabaseResult<()> {
        warn!(
            current_min = self.connector.pool().options().get_min_connections(),
            current_max = self.connector.pool().options().get_max_connections(),
            requested_min = min,
            requested_max = max,
            "Pool resize requested but SQLx does not support dynamic resizing. \
             To change pool size, restart the application with new configuration. \
             See documentation for workarounds."
        );
        Ok(())
    }
}

impl SqlPool {
    /// Returns a repository for the given model type.
    pub fn repository<M>(&self) -> SqlRepository<M>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    {
        let repo = SqlRepository::new(self.connector.clone()).with_schema(self.schema.clone());
        let repo = match &self.key_manager {
            Some(km) => repo.with_key_manager(km.clone()),
            None => repo,
        };
        match &self.metrics {
            Some(metrics) => {
                repo.with_metrics(Arc::clone(metrics), self.config.slow_query_threshold)
            }
            None => repo,
        }
    }

    /// Attaches a key manager, so every repository and transaction this pool hands out encrypts `Secret` columns.
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

    /// Returns a query executor for the given model type; unlike [`repository`](Self::repository), it needs only `FromRow`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use fundamental_db::{Filter, OrderBy, QueryBuilder, QueryExecutor};
    /// use fundamental_core::model::SqlDb;
    /// # #[derive(Clone, fundamental_macros::Model)]
    /// # #[model(sql, table = "users")]
    /// # struct User {
    /// #     #[model(id)]
    /// #     id: uuid::Uuid,
    /// #     role: String,
    /// #     created_at: i64,
    /// # }
    /// # async fn example(pool: &fundamental_sql::SqlPool) -> Result<(), Box<dyn std::error::Error>> {
    /// let executor = pool.query_executor::<User>();
    /// let query = QueryBuilder::<User, SqlDb>::new()
    ///     .filter(Filter::eq("role", "admin"))
    ///     .order_by(OrderBy::desc("created_at"))
    ///     .limit(50);
    ///
    /// let users = executor.execute(&query).await?;
    /// let total = executor.count(&query).await?;
    /// # let _ = (users, total);
    /// # Ok(())
    /// # }
    /// ```
    pub fn query_executor<M>(&self) -> SqlQueryExecutor<M>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + Send + Sync,
    {
        let executor =
            SqlQueryExecutor::new(self.connector.clone()).with_schema(self.schema.clone());
        let executor = match &self.key_manager {
            Some(km) => executor.with_key_manager(km.clone()),
            None => executor,
        };
        match &self.metrics {
            Some(metrics) => {
                executor.with_metrics(Arc::clone(metrics), self.config.slow_query_threshold)
            }
            None => executor,
        }
    }
}

impl Transactional<SqlDb> for SqlPool {
    type Transaction = SqlTransaction;

    #[instrument(
        level = "debug",
        skip(self),
        fields(op = "begin", isolation = %options.isolation_level.as_sql(), read_only = options.read_only, timeout_ms = options.timeout_ms),
        err(Debug)
    )]
    async fn begin_with(&self, options: TransactionOptions) -> DatabaseResult<Self::Transaction> {
        let mut tx = self
            .connector
            .pool()
            .begin()
            .await
            .map_err(SqlError::from)?;

        // Always stated: a bare `BEGIN` inherits a server default that differs per engine.
        // `READ ONLY` in the same statement, so nothing can write before it takes effect.
        let mut sql = format!(
            "SET TRANSACTION ISOLATION LEVEL {}",
            options.isolation_level.as_sql()
        );
        if options.read_only {
            sql.push_str(" READ ONLY");
        }
        if options.deferrable {
            sql.push_str(" DEFERRABLE");
        }
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut *tx)
            .await
            .map_err(SqlError::from)?;
        // `statement_timeout`, not `lock_timeout`: YSQL waits out a row lock under `lock_timeout`.
        if let Some(timeout_ms) = options.timeout_ms {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "SET LOCAL statement_timeout = {timeout_ms}"
            )))
            .execute(&mut *tx)
            .await
            .map_err(SqlError::from)?;
        }

        Ok(self
            .attach_pool_context(SqlTransaction::new(tx).with_isolation(options.isolation_level)))
    }
}

/// `REPEATABLE READ`: YSQL silently ignores `SKIP LOCKED` under `SERIALIZABLE`.
const CLAIM_ISOLATION: IsolationLevel = IsolationLevel::RepeatableRead;

// The only place the clause and an isolation level meet, so a change back to `SERIALIZABLE` fails to compile.
const _: () = assert!(
    matches!(
        CLAIM_ISOLATION,
        IsolationLevel::RepeatableRead | IsolationLevel::ReadCommitted
    ),
    "SKIP LOCKED is warned about and then ignored under SERIALIZABLE on YSQL, which \
     silently degrades the outbox claim to a blocking lock"
);

// No `R::Id` bound: the lease commit addresses each row by its whole key, so tenancy-keyed tables work.
impl<R> OutboxClaimer<R, SqlDb> for SqlPool
where
    R: OutboxRow<SqlDb> + SqlModel + FromRow + ToRow + Clone,
    R::Id: Send + Sync,
{
    #[instrument(level = "debug", skip(self), fields(op = "outbox_claim", table = R::TABLE, batch, claimed = Empty, set_aside = Empty, skipped = Empty), err(Debug))]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    async fn claim(
        &self,
        scope: &R::Scope,
        batch: u64,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<OutboxClaim<R>, AnyError> {
        // Only a process that provably holds its declared keys may blame a row that fails to authenticate.
        let keys_declared = self
            .key_manager
            .as_deref()
            .is_some_and(KeyManager::fingerprints_declared);
        let claim = self
            .transaction(CLAIM_ISOLATION, async |tx| {
                // `SKIP LOCKED` makes two drains take disjoint work; the framework adds the due filters itself.
                let due = || {
                    R::pending(scope)
                        .filter(Filter::lte(R::next_attempt_column(), now))
                        .filter(Filter::is_null(OutboxColumns::SET_ASIDE_AT))
                };
                // Never-skipped rows first, so rows nobody can open never delay one that decodes.
                let fresh = SqlQuery::new(
                    due()
                        .filter(Filter::is_null(OutboxColumns::SKIP_UNTIL))
                        .order_by(OrderBy::asc(R::next_attempt_column()))
                        .limit(batch),
                )
                .for_update_skip_locked();
                let mut read = tx.query_claimable(&fresh).await?;
                let room = batch.saturating_sub(u64::try_from(read.len()).unwrap_or(u64::MAX));
                if room > 0 {
                    let skipped_before = SqlQuery::new(
                        due()
                            .filter(Filter::lte(OutboxColumns::SKIP_UNTIL, now))
                            .order_by(OrderBy::asc(OutboxColumns::SKIP_UNTIL))
                            .limit(room),
                    )
                    .for_update_skip_locked();
                    read.extend(tx.query_claimable(&skipped_before).await?);
                }

                let mut claim = OutboxClaim::empty();
                let mut refused = Vec::new();
                for row in read {
                    match row {
                        ClaimedRow::Decoded(row) => claim.rows.push(row),
                        ClaimedRow::Refused { key, column, kind } => {
                            refused.push((key, column, kind));
                        }
                    }
                }

                for row in &mut claim.rows {
                    row.defer_until(lease_until);
                }
                // The lease commits before relaying, so a slow destination cannot hold a transaction open.
                if !claim.rows.is_empty() {
                    let leased = RepositoryTransactionBatch::update_many(tx, &claim.rows).await?;
                    if leased.len() != claim.rows.len() {
                        return Err(DatabaseError::from(SqlError::OutboxMarkMissed {
                            table: R::TABLE,
                            affected: u64::try_from(leased.len()).unwrap_or(u64::MAX),
                        }));
                    }
                }

                for (key, column, kind) in &refused {
                    match kind {
                        RefusalKind::EnvelopeRefused | RefusalKind::ValueRefused => {
                            tx.set_aside_claimed::<R>(key, *kind, now).await?;
                            claim.set_aside.record(*kind);
                        }
                        RefusalKind::DecryptRefused if keys_declared => {
                            tx.set_aside_claimed::<R>(key, *kind, now).await?;
                            claim.set_aside.record(*kind);
                        }
                        // Another process may hold keys this one lacks, so the row is left for it, never set aside.
                        _ => {
                            tx.skip_claimed::<R>(key, lease_until).await?;
                            claim.skipped += 1;
                        }
                    }
                    warn!(
                        table = R::TABLE,
                        column,
                        reason = kind.code(),
                        "outbox row refused to decode at claim"
                    );
                }
                Ok::<_, DatabaseError>(claim)
            })
            .await
            .map_err(AnyError::new)?;

        let span = Span::current();
        span.record("claimed", claim.rows.len());
        span.record("set_aside", claim.set_aside.total());
        span.record("skipped", claim.skipped);
        Ok(claim)
    }

    #[instrument(level = "debug", skip(self, row), fields(op = "outbox_record_failure", table = R::TABLE, %leased_until, written = Empty), err(Debug))]
    async fn record_failure(
        &self,
        scope: &R::Scope,
        row: &R,
        leased_until: DateTime<Utc>,
    ) -> Result<bool, AnyError> {
        // Anything else means the row moved on while it was relayed.
        let fence = R::pending(scope)
            .filter(Filter::is_null(OutboxColumns::SET_ASIDE_AT))
            .filter(Filter::eq(R::next_attempt_column(), leased_until));
        let written = self
            .transaction(CLAIM_ISOLATION, async |tx| {
                tx.update_fenced(row, &fence).await
            })
            .await
            .map_err(AnyError::new)?
            .is_some();
        Span::current().record("written", written);
        Ok(written)
    }
}

impl<R> OutboxSetAside<R, SqlDb> for SqlPool
where
    R: OutboxRow<SqlDb> + SqlModel + FromRow + ToRow + Clone,
    R::Id: Send + Sync,
{
    #[instrument(level = "debug", skip(self, policy), fields(op = "outbox_inventory", table = R::TABLE, cap, read = Empty, unchecked = Empty), err(Debug))]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    async fn inventory(
        &self,
        scope: &R::Scope,
        now: DateTime<Utc>,
        policy: RetryPolicy,
        cap: u64,
    ) -> Result<OutboxInventory, AnyError> {
        let manager = self.key_manager.clone().ok_or_else(|| {
            AnyError::new(SqlError::Config(format!(
                "the inventory of {} needs a key manager to check set-aside seals",
                R::TABLE
            )))
        })?;
        // A recurring row sits on the horizon after every relay, so clock skew is allowed here.
        let horizon = policy
            .deferral_horizon()
            .and_then(|span| TimeDelta::from_std(span).ok())
            .and_then(|span| span.checked_add(&CLOCK_SKEW))
            .and_then(|span| now.checked_add_signed(span))
            .ok_or_else(|| {
                AnyError::new(SqlError::Config(format!(
                    "the retry policy for {} reaches past any representable instant",
                    R::TABLE
                )))
            })?;
        let skip_horizon = TimeDelta::from_std(policy.lease())
            .ok()
            .and_then(|lease| lease.checked_add(&CLOCK_SKEW))
            .and_then(|span| now.checked_add_signed(span))
            .ok_or_else(|| {
                AnyError::new(SqlError::Config(format!(
                    "the lease for {} reaches past any representable instant",
                    R::TABLE
                )))
            })?;

        let (rows, beyond_policy, skipped, forged_skips) = self
            .transaction(CLAIM_ISOLATION, async |tx| {
                let rows = tx.read_set_aside(&R::pending(scope), cap).await?;
                let deferred = R::pending(scope)
                    .filter(Filter::is_null(OutboxColumns::SET_ASIDE_AT))
                    .filter(Filter::is_null(OutboxColumns::SKIP_UNTIL))
                    .filter(Filter::gt(R::next_attempt_column(), horizon));
                let deferred = match R::due_column() {
                    Some(due) => deferred.filter(Filter::lte(due, now)),
                    None => deferred,
                };
                let beyond_policy = tx.count_held(&deferred, cap).await?;
                let skipped = tx
                    .count_held(
                        &R::pending(scope)
                            .filter(Filter::is_null(OutboxColumns::SET_ASIDE_AT))
                            .filter(Filter::lte(OutboxColumns::SKIP_UNTIL, skip_horizon)),
                        cap,
                    )
                    .await?;
                let forged_skips = tx
                    .count_held(
                        &R::pending(scope)
                            .filter(Filter::is_null(OutboxColumns::SET_ASIDE_AT))
                            .filter(Filter::gt(OutboxColumns::SKIP_UNTIL, skip_horizon)),
                        cap,
                    )
                    .await?;
                Ok::<_, DatabaseError>((rows, beyond_policy, skipped, forged_skips))
            })
            .await
            .map_err(AnyError::new)?;
        if !forged_skips.is_none_held() {
            warn!(
                table = R::TABLE,
                forged_skips = forged_skips.at_least(),
                "outbox rows are skipped further out than any claim writes; counted as unexplained"
            );
        }

        let mut counted: BTreeMap<SetAsideBucket, u64> = BTreeMap::new();
        let mut unchecked = 0_u64;
        for row in &rows {
            let (bucket, checked) = row.bucket(&manager, R::TABLE, now);
            *counted.entry(bucket).or_default() += 1;
            if !checked {
                unchecked += 1;
            }
        }
        if unchecked > 0 {
            warn!(
                table = R::TABLE,
                unchecked,
                "set-asides sealed under a key version this process does not hold were counted under their stated reasons"
            );
        }
        // A read that stopped at its cap cannot say a bucket it saw nothing in is empty.
        let read = u64::try_from(rows.len()).unwrap_or(u64::MAX);
        let held = |count: u64| {
            if read >= cap {
                HeldCount::AtLeast(count)
            } else {
                HeldCount::Exact(count)
            }
        };
        let set_aside = SetAsideBucket::all()
            .map(|bucket| {
                let count = held(counted.get(&bucket).copied().unwrap_or(0));
                match bucket {
                    SetAsideBucket::Unexplained => (bucket, count.plus(forged_skips)),
                    SetAsideBucket::Reason(_) => (bucket, count),
                }
            })
            .collect();

        let span = Span::current();
        span.record("read", read);
        span.record("unchecked", unchecked);
        Ok(OutboxInventory::new(set_aside, beyond_policy, skipped))
    }

    #[instrument(level = "debug", skip(self), fields(op = "outbox_requeue", table = R::TABLE, batch, requeued = Empty), err(Debug))]
    async fn requeue(&self, scope: &R::Scope, batch: u64) -> Result<u64, AnyError> {
        let requeued = self
            .transaction(CLAIM_ISOLATION, async |tx| {
                tx.requeue_set_aside(&R::pending(scope), batch).await
            })
            .await
            .map_err(AnyError::new)?;
        if requeued > 0 {
            info!(table = R::TABLE, requeued, "outbox rows re-queued");
        }
        Span::current().record("requeued", requeued);
        Ok(requeued)
    }
}

impl SqlPool {
    /// Attaches this pool's key manager and metrics to a freshly begun transaction.
    fn attach_pool_context(&self, tx: SqlTransaction) -> SqlTransaction {
        let tx = tx.with_schema(self.schema.clone());
        let tx = match &self.key_manager {
            Some(km) => tx.with_key_manager(km.clone()),
            None => tx,
        };
        match &self.metrics {
            Some(metrics) => tx.with_metrics(Arc::clone(metrics)),
            None => tx,
        }
    }
}

/// How many attempts a DDL statement gets: a floor, so a larger configured budget is kept.
fn ddl_attempts(query_attempts: u32) -> u32 {
    query_attempts.max(DDL_ATTEMPTS)
}

fn calculate_retry_delay_internal(
    base_delay: Duration,
    max_delay: Duration,
    attempt: u32,
) -> Duration {
    let base = base_delay.as_millis() as u64;
    let max = max_delay.as_millis() as u64;

    let delay_ms = base.saturating_mul(1u64 << (attempt - 1).min(10));

    let jitter = (delay_ms / 4) as i64;
    let jittered = if jitter > 0 {
        let random_jitter = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64)
            % (jitter * 2)
            - jitter;
        (delay_ms as i64 + random_jitter).max(0) as u64
    } else {
        delay_ms
    };

    Duration::from_millis(jittered.min(max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_lazy_pool_is_made_without_a_database_and_refuses_only_when_used() {
        let config = SqlConfig::development("postgresql://nobody@127.0.0.1:1/none?sslmode=disable")
            .acquire_timeout(Duration::from_millis(200));
        let pool = SqlPool::connect_lazy(&config).expect("a lazy pool needs no server");
        assert!(pool.connector().acquire().await.is_err());
    }

    #[test]
    fn test_retry_delay_calculation() {
        let base = Duration::from_millis(10);
        let max = Duration::from_millis(1000);

        let delay1 = calculate_retry_delay_internal(base, max, 1);
        assert!(delay1.as_millis() <= 20); // base + jitter

        let delay2 = calculate_retry_delay_internal(base, max, 2);
        assert!(delay2.as_millis() <= 40);

        let delay_max = calculate_retry_delay_internal(base, max, 20);
        assert!(delay_max <= Duration::from_millis(1000));
    }

    #[test]
    fn ddl_does_not_inherit_the_query_budget() {
        let query_attempts = SqlConfig::development("postgres://x/y").max_retries + 1;
        assert!(
            ddl_attempts(query_attempts) > query_attempts,
            "DDL must get more attempts than a query: query had {query_attempts}"
        );
        assert_eq!(ddl_attempts(query_attempts), DDL_ATTEMPTS);
    }

    #[test]
    fn a_larger_configured_budget_is_a_floor_not_an_override() {
        assert_eq!(ddl_attempts(DDL_ATTEMPTS + 100), DDL_ATTEMPTS + 100);
        assert_eq!(ddl_attempts(DDL_ATTEMPTS), DDL_ATTEMPTS);
    }

    #[test]
    fn ddl_retrying_is_bounded_in_time_and_not_only_in_attempts() {
        assert!(
            DDL_TIME_BUDGET < Duration::from_secs(100),
            "three sequential DDL statements must fit inside a 300 s caller"
        );
        assert!(
            DDL_TIME_BUDGET > Duration::from_secs(10),
            "and must still outlast the contention the attempt budget exists for"
        );
    }

    #[test]
    fn no_configuration_can_take_ddl_below_the_floor() {
        for query_attempts in [0, 1, 4, DDL_ATTEMPTS - 1] {
            assert_eq!(ddl_attempts(query_attempts), DDL_ATTEMPTS);
        }
    }
}

fn pool_options(config: &SqlConfig) -> PgPoolOptions {
    let statement_timeout = config.pool.statement_timeout;
    let search_path = config.default_schema.as_str().to_owned();
    PgPoolOptions::new()
        .min_connections(config.pool.min_connections)
        .max_connections(config.pool.max_connections)
        .acquire_timeout(config.pool.acquire_timeout)
        .idle_timeout(config.pool.idle_timeout)
        .max_lifetime(config.pool.max_lifetime)
        .test_before_acquire(config.pool.test_on_checkout)
        .after_connect(move |conn, _meta| {
            let search_path = search_path.clone();
            Box::pin(async move {
                if let Some(timeout) = statement_timeout {
                    let timeout_ms = timeout.as_millis() as i64;
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "SET statement_timeout = {}",
                        timeout_ms
                    )))
                    .execute(&mut *conn)
                    .await?;
                    debug!(timeout_ms = timeout_ms, "Set statement timeout");
                }
                // For what cannot name the schema itself (hand-written DDL, the ledger); a validated identifier.
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "SET search_path = {search_path}"
                )))
                .execute(&mut *conn)
                .await?;
                Ok(())
            })
        })
}

fn reserved_config(parent: &SqlConfig, reservation: &SqlReservation) -> SqlConfig {
    let mut config = parent.clone();
    config.pool.min_connections = reservation.connections();
    config.pool.max_connections = reservation.connections();
    config.pool.acquire_timeout = reservation.acquire_timeout();
    config.pool.application_name = Some(reservation.name().to_owned());
    config
}

/// Builds the connection options for `pool`, applying its TLS posture where the URL left it unsaid.
///
/// # Errors
/// [`SqlError::Config`] if the URL cannot be parsed as PostgreSQL connection options.
fn connect_options(pool: &fundamental_db::PoolConfig) -> SqlResult<PgConnectOptions> {
    use sqlx::postgres::PgSslMode;
    use std::str::FromStr;

    let url = pool.url.expose();
    let mut options = PgConnectOptions::from_str(url)
        .map_err(|error| SqlError::Config(format!("invalid YSQL URL: {error}")))?;

    let url_states_ssl_mode = url.contains("sslmode=");
    if !url_states_ssl_mode {
        options = options.ssl_mode(match pool.tls.mode {
            // A sidecar carries the encrypted hop, so this socket is plaintext on purpose.
            TlsMode::Disabled => PgSslMode::Disable,
            #[cfg(debug_assertions)]
            TlsMode::Preferred => PgSslMode::Prefer,
            TlsMode::Required => PgSslMode::Require,
            TlsMode::VerifyCa => PgSslMode::VerifyCa,
            TlsMode::VerifyFull => PgSslMode::VerifyFull,
        });
    }

    // A configured anchor raises the mode to verify, unless the URL stated one; inline PEM wins over a path.
    let anchor_applied = if let Some(ref ca_cert_pem) = pool.tls.ca_cert_pem {
        options = options.ssl_root_cert_from_pem(ca_cert_pem.as_bytes().to_vec());
        true
    } else if let Some(ref ca_cert_path) = pool.tls.ca_cert_path
        && !url.contains("sslrootcert=")
    {
        options = options.ssl_root_cert(ca_cert_path);
        true
    } else {
        false
    };
    if anchor_applied && !url_states_ssl_mode && pool.tls.mode.is_enabled() {
        options = options.ssl_mode(PgSslMode::VerifyFull);
    }

    // A `cert`-authenticated cluster identifies the SQL user by the certificate's common name.
    if pool.tls.has_client_cert() {
        match (&pool.tls.client_cert_pem, &pool.tls.client_key_pem) {
            (Some(cert), Some(key)) => {
                options = options
                    .ssl_client_cert_from_pem(cert.as_bytes())
                    .ssl_client_key_from_pem(key.as_bytes());
            }
            _ => {
                if let (Some(cert), Some(key)) =
                    (&pool.tls.client_cert_path, &pool.tls.client_key_path)
                {
                    options = options.ssl_client_cert(cert).ssl_client_key(key);
                }
            }
        }
    }

    if let Some(name) = &pool.application_name {
        options = options.application_name(name);
    }

    Ok(options)
}

#[cfg(test)]
mod connect_options_tests {
    use super::*;
    use fundamental_db::{PoolConfig, TlsConfig};

    fn pool_with(tls: TlsConfig) -> PoolConfig {
        let mut config = PoolConfig::new("postgresql://yugabyte@db:5433/layer0");
        config.tls = tls;
        config
    }

    #[test]
    fn a_configured_ca_verifies_rather_than_merely_encrypting() {
        let options = connect_options(&pool_with(TlsConfig::verify_full("/certs/ca.crt")))
            .expect("the URL parses");

        assert_eq!(format!("{:?}", options.get_ssl_mode()), "VerifyFull");
    }

    #[test]
    fn terminated_upstream_reaches_the_driver_as_plaintext() {
        let options =
            connect_options(&pool_with(TlsConfig::terminated_upstream())).expect("the URL parses");

        assert_eq!(format!("{:?}", options.get_ssl_mode()), "Disable");
    }

    #[test]
    fn an_explicit_url_parameter_wins_over_the_posture() {
        let mut config = PoolConfig::new("postgresql://yugabyte@db:5433/layer0?sslmode=disable");
        config.tls = TlsConfig::verify_full("/certs/ca.crt");

        let options = connect_options(&config).expect("the URL parses");

        assert_eq!(format!("{:?}", options.get_ssl_mode()), "Disable");
    }

    #[test]
    fn a_client_certificate_pair_reaches_the_driver() {
        let tls = TlsConfig::verify_full("/certs/ca.crt")
            .client_cert("/certs/client.layer1.crt", "/certs/client.layer1.key");

        let options = connect_options(&pool_with(tls)).expect("the URL parses");

        let rendered = format!("{options:?}");
        assert!(
            rendered.contains("client.layer1.crt") && rendered.contains("client.layer1.key"),
            "both halves must reach the driver: {rendered}"
        );
    }

    #[test]
    fn a_client_certificate_without_its_key_is_not_applied() {
        let mut tls = TlsConfig::verify_full("/certs/ca.crt");
        tls.client_cert_path = Some("/certs/client.layer1.crt".to_string());

        let options = connect_options(&pool_with(tls)).expect("the URL parses");

        assert!(
            !format!("{options:?}").contains("client.layer1.crt"),
            "a certificate with no key must not be handed to the driver"
        );
    }

    #[test]
    fn an_inline_ca_anchor_still_verifies() {
        let options = connect_options(&pool_with(TlsConfig::verify_full_pem(
            "-----BEGIN CERTIFICATE-----\nnot a real certificate\n-----END CERTIFICATE-----",
        )))
        .expect("the URL parses");

        assert_eq!(format!("{:?}", options.get_ssl_mode()), "VerifyFull");
    }

    #[test]
    fn an_unparseable_url_is_a_config_error_not_a_panic() {
        let config = PoolConfig::new("not a url");
        assert!(connect_options(&config).is_err());
    }

    #[test]
    fn a_configured_application_name_reaches_the_driver() {
        let config = PoolConfig::new("postgresql://yugabyte@db:5433/layer0")
            .application_name("deadlines-pulse");
        let options = connect_options(&config).expect("the URL parses");

        assert_eq!(options.get_application_name(), Some("deadlines-pulse"));
    }
}

#[cfg(test)]
mod reservation_tests {
    use std::time::Duration;

    use fundamental_test_containers::sql::{SqlRole, SqlSchema};
    use sqlx::Row as _;

    use super::*;

    async fn pool_on(schema: &SqlSchema, max_connections: u32) -> SqlPool {
        let mut config =
            SqlConfig::development(schema.url()).default_schema(schema.schema().clone());
        config.pool.min_connections = 1;
        config.pool.max_connections = max_connections;
        config.pool.acquire_timeout = Duration::from_millis(300);
        SqlPool::connect(&config)
            .await
            .expect("the shared node serves")
    }

    async fn current_user(pool: &SqlPool) -> String {
        pool.fetch_one("SELECT current_user::text")
            .await
            .expect("the pool serves")
            .try_get::<String, _>(0)
            .expect("a role name")
    }

    #[tokio::test]
    async fn a_reservation_serves_while_the_parent_is_exhausted() {
        let schema = SqlSchema::mint()
            .await
            .expect("a schema on the shared node");
        let parent = pool_on(&schema, 1).await;
        let reserved = parent
            .reserve(&SqlReservation::new("pulse", 1).expect("in bounds"))
            .await
            .expect("the reservation opens");

        let _held = parent
            .connector()
            .acquire()
            .await
            .expect("the parent's one connection");
        let starved = parent.connector().acquire().await;
        assert!(starved.is_err(), "the parent is exhausted");

        let row = reserved
            .fetch_one("SELECT 1::int4")
            .await
            .expect("the reservation still serves");
        assert_eq!(row.try_get::<i32, _>(0).expect("an int"), 1);

        let named = reserved
            .fetch_one("SELECT current_setting('application_name')")
            .await
            .expect("the reservation serves");
        assert_eq!(named.try_get::<String, _>(0).expect("a name"), "pulse");
    }

    #[tokio::test]
    async fn a_rotation_rebuilds_every_reservation() {
        let first = SqlRole::mint().await.expect("a first role");
        let second = SqlRole::mint().await.expect("a second role");
        let parent = SqlPool::connect(&SqlConfig::development(first.url().expose()))
            .await
            .expect("the first role connects");
        let reserved = parent
            .reserve(&SqlReservation::new("pulse", 1).expect("in bounds"))
            .await
            .expect("the reservation opens");
        let dropped = parent
            .reserve(&SqlReservation::new("dropped", 1).expect("in bounds"))
            .await
            .expect("a second reservation opens");
        assert_eq!(current_user(&reserved).await, first.name());
        drop(dropped);

        let fresh = SqlPool::connect(&SqlConfig::development(second.url().expose()))
            .await
            .expect("the second role connects");
        let replaced = parent.rotate_to(&fresh);

        assert_eq!(
            replaced.reserved(),
            1,
            "a dropped reservation is not rebuilt"
        );
        assert_eq!(current_user(&parent).await, second.name());
        assert_eq!(current_user(&reserved).await, second.name());
        let after = parent
            .reserve(&SqlReservation::new("after", 1).expect("in bounds"))
            .await
            .expect("a reservation opened after the rotation");
        assert_eq!(
            current_user(&after).await,
            second.name(),
            "it opens on the rotated credential"
        );
        for pool in replaced.into_pools() {
            pool.close().await;
        }
        first.release().await.expect("the first role drops");
        second.release().await.expect("the second role drops");
    }
}
