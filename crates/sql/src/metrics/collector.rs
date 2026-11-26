use super::SqlMetricsSnapshot;
use crate::SqlMetricsConfig;
use fundamental_db::{DatabaseMetrics, LatencyHistogram, MetricsSnapshot, PercentileSnapshot};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// YSQL metrics collector, on atomic counters.
pub struct SqlMetrics {
    /// Configuration controlling which metrics are tracked and slow-query thresholds.
    pub config: SqlMetricsConfig,
    /// Total number of queries executed.
    pub queries_total: AtomicU64,
    /// Number of queries that returned an error.
    pub queries_failed: AtomicU64,
    /// Number of queries exceeding the slow-query threshold.
    pub slow_queries: AtomicU64,
    /// Cumulative query execution time, in microseconds.
    pub total_query_time_us: AtomicU64,
    /// Total number of transactions started.
    pub transactions_total: AtomicU64,
    /// Number of transactions committed.
    pub transactions_committed: AtomicU64,
    /// Number of transactions rolled back.
    pub transactions_rolled_back: AtomicU64,
    /// Number of serialization conflicts (SQLSTATE 40001) observed.
    pub serialization_conflicts: AtomicU64,
    /// Number of deadlocks (SQLSTATE 40P01) detected.
    pub deadlocks_detected: AtomicU64,
    /// Number of transaction retry attempts.
    pub retry_attempts: AtomicU64,
    /// Retry budgets spent without success.
    pub retry_budget_exhausted: AtomicU64,
    /// Number of connection acquisitions from the pool.
    pub connection_acquires: AtomicU64,
    /// Number of connection acquisitions that timed out.
    pub connection_timeouts: AtomicU64,
    /// Cumulative connection-acquire wait time, in microseconds.
    pub total_acquire_time_us: AtomicU64,
    /// Number of savepoints created (nested transactions).
    pub savepoints_created: AtomicU64,
    /// Number of savepoints rolled back.
    pub savepoints_rolled_back: AtomicU64,
    /// Number of connection-leak warnings emitted.
    pub leak_warnings: AtomicU64,
    /// Histogram of query latencies for percentile reporting.
    pub query_latency: LatencyHistogram,
}

impl std::fmt::Debug for SqlMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlMetrics")
            .field("config", &self.config)
            .field("queries_total", &self.queries_total.load(Ordering::Relaxed))
            .field(
                "queries_failed",
                &self.queries_failed.load(Ordering::Relaxed),
            )
            .field("slow_queries", &self.slow_queries.load(Ordering::Relaxed))
            .field(
                "transactions_total",
                &self.transactions_total.load(Ordering::Relaxed),
            )
            .field(
                "serialization_conflicts",
                &self.serialization_conflicts.load(Ordering::Relaxed),
            )
            .field(
                "deadlocks_detected",
                &self.deadlocks_detected.load(Ordering::Relaxed),
            )
            .field("leak_warnings", &self.leak_warnings.load(Ordering::Relaxed))
            .finish()
    }
}

impl SqlMetrics {
    /// Creates a new metrics collector with the given configuration.
    pub fn new(config: SqlMetricsConfig) -> Self {
        Self {
            config,
            queries_total: AtomicU64::new(0),
            queries_failed: AtomicU64::new(0),
            slow_queries: AtomicU64::new(0),
            total_query_time_us: AtomicU64::new(0),
            transactions_total: AtomicU64::new(0),
            transactions_committed: AtomicU64::new(0),
            transactions_rolled_back: AtomicU64::new(0),
            serialization_conflicts: AtomicU64::new(0),
            deadlocks_detected: AtomicU64::new(0),
            retry_attempts: AtomicU64::new(0),
            retry_budget_exhausted: AtomicU64::new(0),
            connection_acquires: AtomicU64::new(0),
            connection_timeouts: AtomicU64::new(0),
            total_acquire_time_us: AtomicU64::new(0),
            savepoints_created: AtomicU64::new(0),
            savepoints_rolled_back: AtomicU64::new(0),
            leak_warnings: AtomicU64::new(0),
            query_latency: LatencyHistogram::new(),
        }
    }

    /// Creates a metrics collector with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(SqlMetricsConfig::default())
    }

    /// Returns the configuration.
    pub fn config(&self) -> &SqlMetricsConfig {
        &self.config
    }

    /// Wraps an async operation with timing and metrics collection.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use fundamental_sql::{SqlMetrics, SqlMetricsConfig};
    /// # use fundamental_db::Repository;
    /// # #[derive(Clone, fundamental_macros::Model)]
    /// # #[model(sql, table = "users")]
    /// # struct User {
    /// #     #[model(id)]
    /// #     id: uuid::Uuid,
    /// # }
    /// # async fn example(
    /// #     pool: &fundamental_sql::SqlPool,
    /// #     user: User,
    /// # ) -> Result<(), Box<dyn std::error::Error>> {
    /// # let metrics = SqlMetrics::new(SqlMetricsConfig::default());
    /// # let repository = pool.repository::<User>();
    /// let result = metrics.timed("insert", "users", async {
    ///     repository.insert(&user).await
    /// }).await;
    /// # let _ = result;
    /// # Ok(())
    /// # }
    /// ```
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn timed<F, T, E>(&self, operation: &str, table: &str, f: F) -> Result<T, E>
    where
        F: Future<Output = Result<T, E>>,
    {
        let start = Instant::now();
        let result = f.await;
        let duration = start.elapsed();

        self.record_query(operation, table, duration, result.is_ok());

        if duration > self.config.base.slow_query_threshold {
            self.record_slow_query(operation, table, duration);
        }

        result
    }

    /// Wraps an async operation with timing, including query text in slow query logs.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn timed_with_query<F, T, E>(
        &self,
        operation: &str,
        table: &str,
        query: &str,
        f: F,
    ) -> Result<T, E>
    where
        F: Future<Output = Result<T, E>>,
    {
        let start = Instant::now();
        let result = f.await;
        let duration = start.elapsed();

        self.record_query(operation, table, duration, result.is_ok());

        if duration > self.config.base.slow_query_threshold {
            self.record_slow_query(operation, table, duration);
            if self.config.log_slow_query_text {
                warn!(
                    operation = %operation,
                    table = %table,
                    duration_ms = duration.as_millis(),
                    query = %query,
                    "Slow YSQL query (with query text)"
                );
            }
        }

        result
    }

    /// Records a serialization conflict.
    pub fn record_serialization_conflict(&self) {
        if self.config.track_serialization_conflicts {
            self.serialization_conflicts.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records a deadlock detection (SQLSTATE 40P01).
    pub fn record_deadlock(&self) {
        self.deadlocks_detected.fetch_add(1, Ordering::Relaxed);
        warn!("Deadlock detected");
    }

    /// Records a connection leak warning.
    pub fn record_leak_warning(&self) {
        self.leak_warnings.fetch_add(1, Ordering::Relaxed);
    }

    /// Records one retry budget spent without success; unlabelled here, as the whole snapshot is.
    pub fn record_retry_exhausted(&self, sqlstate: &str) {
        let _ = sqlstate;
        self.retry_budget_exhausted.fetch_add(1, Ordering::Relaxed);
    }

    /// Records one retry attempt under the SQLSTATE that caused it, moving the per-class counters too.
    pub fn record_retry(&self, sqlstate: &str) {
        self.retry_attempts.fetch_add(1, Ordering::Relaxed);
        match sqlstate {
            "40001" => self.record_serialization_conflict(),
            "40P01" => self.record_deadlock(),
            _ => {}
        }
    }

    /// Returns query latency percentiles.
    pub fn query_percentiles(&self) -> PercentileSnapshot {
        self.query_latency.snapshot()
    }

    /// Records a transaction start.
    pub fn record_transaction_start(&self) {
        if self.config.track_transaction_stats {
            self.transactions_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records a transaction commit.
    pub fn record_transaction_commit(&self, duration: Duration) {
        if self.config.track_transaction_stats {
            self.transactions_committed.fetch_add(1, Ordering::Relaxed);
        }
        debug!(duration_ms = duration.as_millis(), "Transaction committed");
    }

    /// Records a transaction rollback.
    pub fn record_transaction_rollback(&self, duration: Duration) {
        if self.config.track_transaction_stats {
            self.transactions_rolled_back
                .fetch_add(1, Ordering::Relaxed);
        }
        debug!(
            duration_ms = duration.as_millis(),
            "Transaction rolled back"
        );
    }

    /// Records a savepoint creation.
    pub fn record_savepoint_created(&self) {
        self.savepoints_created.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a savepoint rollback.
    pub fn record_savepoint_rollback(&self) {
        self.savepoints_rolled_back.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns a snapshot of current metrics.
    pub fn snapshot(&self) -> SqlMetricsSnapshot {
        SqlMetricsSnapshot {
            base: MetricsSnapshot {
                queries_total: self.queries_total.load(Ordering::Relaxed),
                queries_failed: self.queries_failed.load(Ordering::Relaxed),
                slow_queries: self.slow_queries.load(Ordering::Relaxed),
                total_query_time_us: self.total_query_time_us.load(Ordering::Relaxed),
                transactions_total: self.transactions_total.load(Ordering::Relaxed),
                transactions_committed: self.transactions_committed.load(Ordering::Relaxed),
                serialization_conflicts: self.serialization_conflicts.load(Ordering::Relaxed),
                connection_acquires: self.connection_acquires.load(Ordering::Relaxed),
                connection_timeouts: self.connection_timeouts.load(Ordering::Relaxed),
            },
            transactions_rolled_back: self.transactions_rolled_back.load(Ordering::Relaxed),
            retry_attempts: self.retry_attempts.load(Ordering::Relaxed),
            total_acquire_time_us: self.total_acquire_time_us.load(Ordering::Relaxed),
            savepoints_created: self.savepoints_created.load(Ordering::Relaxed),
            savepoints_rolled_back: self.savepoints_rolled_back.load(Ordering::Relaxed),
            deadlocks_detected: self.deadlocks_detected.load(Ordering::Relaxed),
            leak_warnings: self.leak_warnings.load(Ordering::Relaxed),
            p50_query_us: self.query_latency.p50(),
            p95_query_us: self.query_latency.p95(),
            p99_query_us: self.query_latency.p99(),
        }
    }

    /// Resets all metrics counters to zero.
    pub fn reset(&self) {
        self.queries_total.store(0, Ordering::Relaxed);
        self.queries_failed.store(0, Ordering::Relaxed);
        self.slow_queries.store(0, Ordering::Relaxed);
        self.total_query_time_us.store(0, Ordering::Relaxed);
        self.transactions_total.store(0, Ordering::Relaxed);
        self.transactions_committed.store(0, Ordering::Relaxed);
        self.transactions_rolled_back.store(0, Ordering::Relaxed);
        self.serialization_conflicts.store(0, Ordering::Relaxed);
        self.retry_attempts.store(0, Ordering::Relaxed);
        self.connection_acquires.store(0, Ordering::Relaxed);
        self.connection_timeouts.store(0, Ordering::Relaxed);
        self.total_acquire_time_us.store(0, Ordering::Relaxed);
        self.savepoints_created.store(0, Ordering::Relaxed);
        self.savepoints_rolled_back.store(0, Ordering::Relaxed);
    }
}

impl DatabaseMetrics for SqlMetrics {
    fn record_retry(&self, sqlstate: &str) {
        Self::record_retry(self, sqlstate);
    }

    fn record_retry_exhausted(&self, sqlstate: &str) {
        Self::record_retry_exhausted(self, sqlstate);
    }

    fn record_query(&self, operation: &str, table: &str, duration: Duration, success: bool) {
        self.queries_total.fetch_add(1, Ordering::Relaxed);
        self.total_query_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);

        self.query_latency.record(duration);

        if !success {
            self.queries_failed.fetch_add(1, Ordering::Relaxed);
        }

        debug!(
            operation = %operation,
            table = %table,
            duration_ms = duration.as_millis(),
            success = success,
            "YSQL query executed"
        );
    }

    fn record_connection_acquire(&self, duration: Duration, success: bool) {
        if self.config.track_pool_metrics {
            self.connection_acquires.fetch_add(1, Ordering::Relaxed);
            self.total_acquire_time_us
                .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);

            if !success {
                self.connection_timeouts.fetch_add(1, Ordering::Relaxed);
            }

            if duration > self.config.base.connection_acquire_warning_threshold {
                warn!(
                    duration_ms = duration.as_millis(),
                    threshold_ms = self
                        .config
                        .base
                        .connection_acquire_warning_threshold
                        .as_millis(),
                    "Slow connection acquisition - possible pool contention"
                );
            }
        }
    }

    fn record_slow_query(&self, operation: &str, table: &str, duration: Duration) {
        self.slow_queries.fetch_add(1, Ordering::Relaxed);

        warn!(
            operation = %operation,
            table = %table,
            duration_ms = duration.as_millis(),
            threshold_ms = self.config.base.slow_query_threshold.as_millis(),
            "Slow YSQL query detected"
        );
    }

    fn record_transaction(&self, committed: bool, duration: Duration) {
        if self.config.track_transaction_stats {
            if committed {
                self.record_transaction_commit(duration);
            } else {
                self.record_transaction_rollback(duration);
            }
        }
    }

    fn record_transaction_start(&self) {
        SqlMetrics::record_transaction_start(self);
    }
}

impl Default for SqlMetrics {
    fn default() -> Self {
        Self::with_defaults()
    }
}
