use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use fundamental_db::{DatabaseMetrics, LatencyHistogram, MetricsSnapshot, PercentileSnapshot};
use tracing::{debug, warn};

use crate::{CqlMetricsConfig, CqlMetricsSnapshot};

/// YCQL metrics collector.
pub struct CqlMetrics {
    /// Which metrics are tracked, and the slow-query threshold.
    pub config: CqlMetricsConfig,
    queries_total: AtomicU64,
    queries_failed: AtomicU64,
    slow_queries: AtomicU64,
    total_query_time_us: AtomicU64,
    batch_operations: AtomicU64,
    batch_items_total: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    lwt_operations: AtomicU64,
    lwt_applied: AtomicU64,
    leak_warnings: AtomicU64,
    query_latency: LatencyHistogram,
}

impl std::fmt::Debug for CqlMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CqlMetrics")
            .field("config", &self.config)
            .field("queries_total", &self.queries_total.load(Ordering::Relaxed))
            .field(
                "queries_failed",
                &self.queries_failed.load(Ordering::Relaxed),
            )
            .field("slow_queries", &self.slow_queries.load(Ordering::Relaxed))
            .field(
                "batch_operations",
                &self.batch_operations.load(Ordering::Relaxed),
            )
            .field("cache_hits", &self.cache_hits.load(Ordering::Relaxed))
            .field("leak_warnings", &self.leak_warnings.load(Ordering::Relaxed))
            .finish()
    }
}

impl CqlMetrics {
    /// Creates a new metrics collector with the given configuration.
    pub fn new(config: CqlMetricsConfig) -> Self {
        Self {
            config,
            queries_total: AtomicU64::new(0),
            queries_failed: AtomicU64::new(0),
            slow_queries: AtomicU64::new(0),
            total_query_time_us: AtomicU64::new(0),
            batch_operations: AtomicU64::new(0),
            batch_items_total: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            lwt_operations: AtomicU64::new(0),
            lwt_applied: AtomicU64::new(0),
            leak_warnings: AtomicU64::new(0),
            query_latency: LatencyHistogram::new(),
        }
    }

    /// Returns query latency percentiles.
    pub fn query_percentiles(&self) -> PercentileSnapshot {
        self.query_latency.snapshot()
    }

    /// Creates a metrics collector with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(CqlMetricsConfig::default())
    }

    /// Returns the configuration.
    pub fn config(&self) -> &CqlMetricsConfig {
        &self.config
    }

    /// Wraps an async operation with timing and metrics collection.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use fundamental_db::Repository;
    /// use fundamental_cql::{CqlMetrics, CqlMetricsConfig, CqlPool};
    /// # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
    /// # #[model(cql, table = "events")]
    /// # struct Event {
    /// #     #[model(partition_key)]
    /// #     tenant_id: uuid::Uuid,
    /// #     payload: String,
    /// # }
    /// # async fn example(pool: CqlPool, event: Event) {
    /// let metrics = CqlMetrics::new(CqlMetricsConfig::default());
    /// let repository = pool.repository::<Event>();
    /// let result = metrics.timed("insert", "events", async {
    ///     repository.insert(&event).await
    /// }).await;
    /// # let _ = result;
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

    /// Records a batch operation.
    pub fn record_batch(&self, item_count: u64) {
        if self.config.track_batch_stats {
            self.batch_operations.fetch_add(1, Ordering::Relaxed);
            self.batch_items_total
                .fetch_add(item_count, Ordering::Relaxed);
        }
    }

    /// Records a prepared statement cache hit.
    pub fn record_cache_hit(&self) {
        if self.config.track_cache_stats {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records a prepared statement cache miss.
    pub fn record_cache_miss(&self) {
        if self.config.track_cache_stats {
            self.cache_misses.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records a lightweight transaction (LWT) operation.
    pub fn record_lwt(&self, applied: bool) {
        self.lwt_operations.fetch_add(1, Ordering::Relaxed);
        if applied {
            self.lwt_applied.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records a leak warning (operation exceeded threshold).
    pub fn record_leak_warning(&self) {
        self.leak_warnings.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the number of leak warnings.
    pub fn leak_warning_count(&self) -> u64 {
        self.leak_warnings.load(Ordering::Relaxed)
    }

    /// Returns a snapshot of current metrics.
    pub fn snapshot(&self) -> CqlMetricsSnapshot {
        CqlMetricsSnapshot {
            base: MetricsSnapshot {
                queries_total: self.queries_total.load(Ordering::Relaxed),
                queries_failed: self.queries_failed.load(Ordering::Relaxed),
                slow_queries: self.slow_queries.load(Ordering::Relaxed),
                total_query_time_us: self.total_query_time_us.load(Ordering::Relaxed),
                transactions_total: 0,
                transactions_committed: 0,
                serialization_conflicts: 0,
                connection_acquires: 0,
                connection_timeouts: 0,
            },
            batch_operations: self.batch_operations.load(Ordering::Relaxed),
            batch_items_total: self.batch_items_total.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            lwt_operations: self.lwt_operations.load(Ordering::Relaxed),
            lwt_applied: self.lwt_applied.load(Ordering::Relaxed),
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
        self.batch_operations.store(0, Ordering::Relaxed);
        self.batch_items_total.store(0, Ordering::Relaxed);
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
        self.lwt_operations.store(0, Ordering::Relaxed);
        self.lwt_applied.store(0, Ordering::Relaxed);
        self.leak_warnings.store(0, Ordering::Relaxed);
    }
}

impl DatabaseMetrics for CqlMetrics {
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
            "YCQL query executed"
        );
    }

    fn record_connection_acquire(&self, duration: Duration, success: bool) {
        // YCQL has no traditional pool; this exists for the trait.
        debug!(
            duration_ms = duration.as_millis(),
            success = success,
            "YCQL session operation"
        );
    }

    fn record_slow_query(&self, operation: &str, table: &str, duration: Duration) {
        self.slow_queries.fetch_add(1, Ordering::Relaxed);

        warn!(
            operation = %operation,
            table = %table,
            duration_ms = duration.as_millis(),
            threshold_ms = self.config.base.slow_query_threshold.as_millis(),
            "Slow YCQL query detected"
        );
    }

    fn record_transaction(&self, _committed: bool, _duration: Duration) {
        // YCQL has no traditional transactions; LWTs are tracked separately.
    }
}

impl Default for CqlMetrics {
    fn default() -> Self {
        Self::with_defaults()
    }
}
