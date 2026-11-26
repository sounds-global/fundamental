//! Point-in-time percentile snapshot type.

/// A snapshot of percentile values from a histogram.
#[derive(Debug, Clone, Copy, Default)]
#[cfg(feature = "std")]
pub struct PercentileSnapshot {
    /// Total number of recorded values.
    pub count: u64,
    /// Minimum value in microseconds.
    pub min_us: u64,
    /// Maximum value in microseconds.
    pub max_us: u64,
    /// Mean value in microseconds.
    pub mean_us: f64,
    /// 50th percentile (median) in microseconds.
    pub p50_us: u64,
    /// 95th percentile in microseconds.
    pub p95_us: u64,
    /// 99th percentile in microseconds.
    pub p99_us: u64,
    /// 99.9th percentile in microseconds.
    pub p99_9_us: u64,
}

#[cfg(feature = "std")]
impl PercentileSnapshot {
    /// Returns the 50th percentile in milliseconds.
    pub fn p50_ms(&self) -> f64 {
        self.p50_us as f64 / 1000.0
    }

    /// Returns the 95th percentile in milliseconds.
    pub fn p95_ms(&self) -> f64 {
        self.p95_us as f64 / 1000.0
    }

    /// Returns the 99th percentile in milliseconds.
    pub fn p99_ms(&self) -> f64 {
        self.p99_us as f64 / 1000.0
    }

    /// Returns the 99.9th percentile in milliseconds.
    pub fn p99_9_ms(&self) -> f64 {
        self.p99_9_us as f64 / 1000.0
    }

    /// Returns the mean in milliseconds.
    pub fn mean_ms(&self) -> f64 {
        self.mean_us / 1000.0
    }
}
