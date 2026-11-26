//! Configuration for YSQL metrics collection.

use fundamental_db::MetricsConfig;
use std::time::Duration;

/// YSQL-specific metrics configuration.
#[derive(Debug, Clone)]
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
pub struct SqlMetricsConfig {
    /// Base metrics configuration.
    pub base: MetricsConfig,

    /// Whether to track transaction statistics.
    pub track_transaction_stats: bool,

    /// Whether to track serialization conflicts.
    pub track_serialization_conflicts: bool,

    /// Whether to track connection pool metrics.
    pub track_pool_metrics: bool,

    /// Whether to log query text for slow queries, which may contain sensitive values.
    pub log_slow_query_text: bool,
}

impl Default for SqlMetricsConfig {
    fn default() -> Self {
        Self {
            base: MetricsConfig::default(),
            track_transaction_stats: true,
            track_serialization_conflicts: true,
            track_pool_metrics: true,
            log_slow_query_text: cfg!(debug_assertions),
        }
    }
}

impl SqlMetricsConfig {
    /// Creates a new YSQL metrics configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the slow query threshold.
    pub fn slow_query_threshold(mut self, threshold: Duration) -> Self {
        self.base.slow_query_threshold = threshold;
        self
    }

    /// Sets whether to include query text in slow query logs.
    pub fn include_query_text(mut self, include: bool) -> Self {
        self.base.include_query_text = include;
        self
    }

    /// Sets whether to log query text for slow queries.
    pub fn log_slow_query_text(mut self, enable: bool) -> Self {
        self.log_slow_query_text = enable;
        self
    }

    /// Sets whether to track transaction statistics.
    pub fn track_transaction_stats(mut self, enable: bool) -> Self {
        self.track_transaction_stats = enable;
        self
    }

    /// Sets whether to track serialization conflicts.
    pub fn track_serialization_conflicts(mut self, enable: bool) -> Self {
        self.track_serialization_conflicts = enable;
        self
    }

    /// Sets whether to track connection pool metrics.
    pub fn track_pool_metrics(mut self, enable: bool) -> Self {
        self.track_pool_metrics = enable;
        self
    }

    /// Sets the connection acquisition warning threshold.
    pub fn connection_acquire_warning_threshold(mut self, threshold: Duration) -> Self {
        self.base.connection_acquire_warning_threshold = threshold;
        self
    }
}
