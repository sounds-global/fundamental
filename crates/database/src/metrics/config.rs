//! Metrics collection configuration.

#[cfg(feature = "std")]
use std::time::Duration;

/// Configuration for metrics collection.
#[derive(Debug, Clone)]
#[cfg(feature = "std")]
pub struct MetricsConfig {
    /// Threshold for slow query logging.
    ///
    /// Default: 1 second.
    pub slow_query_threshold: Duration,

    /// Whether to include query text in slow query logs.
    /// Whether to include query text, which may hold sensitive parameters, in slow query logs.
    ///
    /// Default: false in production, true in debug builds.
    pub include_query_text: bool,

    /// Histogram buckets for query duration (in seconds).
    ///
    /// Default: [`DEFAULT_DURATION_BUCKETS`].
    pub duration_buckets: Vec<f64>,

    /// Threshold for connection acquisition warnings.
    ///
    /// Default: 100ms.
    pub connection_acquire_warning_threshold: Duration,

    /// Whether to enable per-table metrics.
    ///
    /// Raises cardinality. Default: true.
    pub per_table_metrics: bool,
}

/// The canonical query-duration histogram buckets, in seconds.
pub const DEFAULT_DURATION_BUCKETS: &[f64] = &[
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

#[cfg(feature = "std")]
impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            slow_query_threshold: Duration::from_secs(1),
            include_query_text: cfg!(debug_assertions),
            duration_buckets: DEFAULT_DURATION_BUCKETS.to_vec(),
            connection_acquire_warning_threshold: Duration::from_millis(100),
            per_table_metrics: true,
        }
    }
}

#[cfg(feature = "std")]
impl MetricsConfig {
    /// Creates a new metrics configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the slow query threshold.
    pub fn slow_query_threshold(mut self, threshold: Duration) -> Self {
        self.slow_query_threshold = threshold;
        self
    }

    /// Sets whether to include query text in slow query logs.
    pub fn include_query_text(mut self, include: bool) -> Self {
        self.include_query_text = include;
        self
    }

    /// Sets the histogram buckets for query duration.
    pub fn duration_buckets(mut self, buckets: Vec<f64>) -> Self {
        self.duration_buckets = buckets;
        self
    }

    /// Sets the connection acquisition warning threshold.
    pub fn connection_acquire_warning_threshold(mut self, threshold: Duration) -> Self {
        self.connection_acquire_warning_threshold = threshold;
        self
    }

    /// Sets whether to enable per-table metrics.
    pub fn per_table_metrics(mut self, enable: bool) -> Self {
        self.per_table_metrics = enable;
        self
    }
}
