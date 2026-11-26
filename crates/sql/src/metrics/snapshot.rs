//! Point-in-time snapshot of YSQL metrics.

use fundamental_db::MetricsSnapshot;

/// YSQL-specific metrics snapshot.
#[derive(Debug, Clone, Default)]
pub struct SqlMetricsSnapshot {
    /// Base metrics.
    pub base: MetricsSnapshot,

    /// Number of transactions that were rolled back.
    pub transactions_rolled_back: u64,

    /// Number of retry attempts due to serialization conflicts.
    pub retry_attempts: u64,

    /// Total time spent acquiring connections (microseconds).
    pub total_acquire_time_us: u64,

    /// Number of savepoints created.
    pub savepoints_created: u64,

    /// Number of savepoints that were rolled back.
    pub savepoints_rolled_back: u64,

    /// Number of deadlocks detected (SQLSTATE 40P01).
    pub deadlocks_detected: u64,

    /// Number of connection leak warnings.
    pub leak_warnings: u64,

    /// 50th percentile query latency in microseconds.
    pub p50_query_us: u64,

    /// 95th percentile query latency in microseconds.
    pub p95_query_us: u64,

    /// 99th percentile query latency in microseconds.
    pub p99_query_us: u64,
}

impl SqlMetricsSnapshot {
    /// Returns the transaction commit rate as a percentage (0.0 to 100.0).
    pub fn transaction_commit_rate(&self) -> f64 {
        self.base.transaction_commit_rate()
    }

    /// Returns the serialization conflict rate as a percentage of transactions.
    pub fn serialization_conflict_rate(&self) -> f64 {
        if self.base.transactions_total == 0 {
            0.0
        } else {
            (self.base.serialization_conflicts as f64 / self.base.transactions_total as f64) * 100.0
        }
    }

    /// Returns the average connection acquisition time in milliseconds.
    pub fn average_acquire_time_ms(&self) -> f64 {
        if self.base.connection_acquires == 0 {
            0.0
        } else {
            (self.total_acquire_time_us as f64 / self.base.connection_acquires as f64) / 1000.0
        }
    }

    /// Returns the connection timeout rate as a percentage.
    pub fn connection_timeout_rate(&self) -> f64 {
        if self.base.connection_acquires == 0 {
            0.0
        } else {
            (self.base.connection_timeouts as f64 / self.base.connection_acquires as f64) * 100.0
        }
    }

    /// Returns the savepoint rollback rate as a percentage.
    pub fn savepoint_rollback_rate(&self) -> f64 {
        if self.savepoints_created == 0 {
            0.0
        } else {
            (self.savepoints_rolled_back as f64 / self.savepoints_created as f64) * 100.0
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

    /// Returns the deadlock rate as a percentage of transactions.
    pub fn deadlock_rate(&self) -> f64 {
        if self.base.transactions_total == 0 {
            0.0
        } else {
            (self.deadlocks_detected as f64 / self.base.transactions_total as f64) * 100.0
        }
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
