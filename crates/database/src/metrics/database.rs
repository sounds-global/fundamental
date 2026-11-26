//! The [`DatabaseMetrics`] trait.

#[cfg(feature = "std")]
use std::time::Duration;

/// Database operation metrics trait.
///
/// All methods are low-overhead and suitable for hot paths.
pub trait DatabaseMetrics: Send + Sync {
    /// Records a query execution.
    #[cfg(feature = "std")]
    fn record_query(&self, operation: &str, table: &str, duration: Duration, success: bool);

    /// Records a connection acquisition from the pool.
    #[cfg(feature = "std")]
    fn record_connection_acquire(&self, duration: Duration, success: bool);

    /// Records a slow query for alerting and analysis.
    ///
    /// Called in addition to `record_query` when a query exceeds the slow query threshold.
    #[cfg(feature = "std")]
    fn record_slow_query(&self, operation: &str, table: &str, duration: Duration);

    /// Records a transaction completion.
    #[cfg(feature = "std")]
    fn record_transaction(&self, committed: bool, duration: Duration);

    /// Records one retry of a transaction, labelled by the SQLSTATE that caused it.
    ///
    /// `sqlstate` is the engine's code, or `"unknown"` where the error carried none. Defaults to
    /// doing nothing.
    #[cfg(feature = "std")]
    fn record_retry(&self, sqlstate: &str) {
        let _ = sqlstate;
    }

    /// Records a retry budget spent without success, labelled by the SQLSTATE that spent it.
    ///
    /// Counted once per exhausted budget, never per attempt; the one to alert on. Defaults to
    /// doing nothing.
    #[cfg(feature = "std")]
    fn record_retry_exhausted(&self, sqlstate: &str) {
        let _ = sqlstate;
    }

    /// Records that a transaction began.
    ///
    /// Defaults to doing nothing; implement it for a collector that reports a gauge.
    #[cfg(feature = "std")]
    fn record_transaction_start(&self) {}
}
