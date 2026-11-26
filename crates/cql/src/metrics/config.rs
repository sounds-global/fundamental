//! Configuration for YCQL metrics collection.

use fundamental_db::MetricsConfig;
use std::time::Duration;

/// YCQL-specific metrics configuration.
#[derive(Debug, Clone)]
pub struct CqlMetricsConfig {
    /// Base metrics configuration.
    pub base: MetricsConfig,

    /// Whether to track prepared statement cache statistics.
    pub track_cache_stats: bool,

    /// Whether to track batch operation statistics.
    pub track_batch_stats: bool,
}

impl Default for CqlMetricsConfig {
    fn default() -> Self {
        Self {
            base: MetricsConfig::default(),
            track_cache_stats: true,
            track_batch_stats: true,
        }
    }
}

impl CqlMetricsConfig {
    /// Creates a new YCQL metrics configuration.
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

    /// Sets whether to track prepared statement cache statistics.
    pub fn track_cache_stats(mut self, enable: bool) -> Self {
        self.track_cache_stats = enable;
        self
    }

    /// Sets whether to track batch operation statistics.
    pub fn track_batch_stats(mut self, enable: bool) -> Self {
        self.track_batch_stats = enable;
        self
    }
}
