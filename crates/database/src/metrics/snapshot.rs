//! Metrics summary snapshot.

/// Summary statistics from metrics collection.
#[derive(Debug, Clone, Default)]
#[cfg(feature = "std")]
pub struct MetricsSnapshot {
    /// Total number of queries executed.
    pub queries_total: u64,

    /// Number of failed queries.
    pub queries_failed: u64,

    /// Number of slow queries detected.
    pub slow_queries: u64,

    /// Total query execution time in microseconds.
    pub total_query_time_us: u64,

    /// Total number of transactions.
    pub transactions_total: u64,

    /// Number of transactions that were committed.
    pub transactions_committed: u64,

    /// Number of serialization conflicts (YSQL).
    pub serialization_conflicts: u64,

    /// Total connection acquisitions.
    pub connection_acquires: u64,

    /// Connection acquisition timeouts.
    pub connection_timeouts: u64,
}

#[cfg(feature = "std")]
impl MetricsSnapshot {
    /// Returns the query success rate as a percentage (0.0 to 100.0).
    pub fn query_success_rate(&self) -> f64 {
        if self.queries_total == 0 {
            100.0
        } else {
            let successful = self.queries_total - self.queries_failed;
            (successful as f64 / self.queries_total as f64) * 100.0
        }
    }

    /// Returns the average query duration in milliseconds.
    pub fn average_query_duration_ms(&self) -> f64 {
        if self.queries_total == 0 {
            0.0
        } else {
            (self.total_query_time_us as f64 / self.queries_total as f64) / 1000.0
        }
    }

    /// Returns the transaction commit rate as a percentage (0.0 to 100.0).
    pub fn transaction_commit_rate(&self) -> f64 {
        if self.transactions_total == 0 {
            100.0
        } else {
            (self.transactions_committed as f64 / self.transactions_total as f64) * 100.0
        }
    }

    /// Returns the slow query rate as a percentage (0.0 to 100.0).
    pub fn slow_query_rate(&self) -> f64 {
        if self.queries_total == 0 {
            0.0
        } else {
            (self.slow_queries as f64 / self.queries_total as f64) * 100.0
        }
    }
}
