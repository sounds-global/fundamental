//! Point-in-time snapshot of collected YCQL metrics.

use fundamental_db::MetricsSnapshot;

/// YCQL-specific metrics snapshot.
#[derive(Debug, Clone, Default)]
pub struct CqlMetricsSnapshot {
    /// Base metrics.
    pub base: MetricsSnapshot,

    /// Number of batch operations executed.
    pub batch_operations: u64,

    /// Total items processed in batches.
    pub batch_items_total: u64,

    /// Prepared statement cache hits.
    pub cache_hits: u64,

    /// Prepared statement cache misses.
    pub cache_misses: u64,

    /// Number of LWT operations.
    pub lwt_operations: u64,

    /// Number of LWT operations that were applied.
    pub lwt_applied: u64,

    /// Number of operations that exceeded the leak detection threshold.
    pub leak_warnings: u64,

    /// 50th percentile query latency in microseconds.
    pub p50_query_us: u64,

    /// 95th percentile query latency in microseconds.
    pub p95_query_us: u64,

    /// 99th percentile query latency in microseconds.
    pub p99_query_us: u64,
}

impl CqlMetricsSnapshot {
    /// Returns the cache hit rate as a percentage (0.0 to 100.0).
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            100.0
        } else {
            (self.cache_hits as f64 / total as f64) * 100.0
        }
    }

    /// Returns the average batch size.
    pub fn average_batch_size(&self) -> f64 {
        if self.batch_operations == 0 {
            0.0
        } else {
            self.batch_items_total as f64 / self.batch_operations as f64
        }
    }

    /// Returns the LWT success rate as a percentage (0.0 to 100.0).
    pub fn lwt_success_rate(&self) -> f64 {
        if self.lwt_operations == 0 {
            100.0
        } else {
            (self.lwt_applied as f64 / self.lwt_operations as f64) * 100.0
        }
    }

    /// Delegates to base metrics.
    pub fn query_success_rate(&self) -> f64 {
        self.base.query_success_rate()
    }

    /// Delegates to base metrics.
    pub fn average_query_duration_ms(&self) -> f64 {
        self.base.average_query_duration_ms()
    }

    /// Delegates to base metrics.
    pub fn slow_query_rate(&self) -> f64 {
        self.base.slow_query_rate()
    }

    /// Returns the 50th percentile query latency in milliseconds.
    pub fn p50_query_ms(&self) -> f64 {
        self.p50_query_us as f64 / 1000.0
    }

    /// Returns the 95th percentile query latency in milliseconds.
    pub fn p95_query_ms(&self) -> f64 {
        self.p95_query_us as f64 / 1000.0
    }

    /// Returns the 99th percentile query latency in milliseconds.
    pub fn p99_query_ms(&self) -> f64 {
        self.p99_query_us as f64 / 1000.0
    }
}
