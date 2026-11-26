//! Thread-safe HDR-histogram-backed latency tracker (`std` only).

#[cfg(feature = "std")]
use parking_lot::Mutex;

#[cfg(feature = "std")]
use std::time::Duration;

#[cfg(feature = "std")]
use tracing::warn;

#[cfg(feature = "std")]
use super::PercentileSnapshot;

/// High-precision latency histogram for percentile calculations.
///
/// Thread-safe through an internal lock held only for each histogram operation.
///
/// # Example
///
/// ```rust
/// use fundamental_db::LatencyHistogram;
/// use std::time::Duration;
///
/// let histogram = LatencyHistogram::new();
///
/// // Record latencies
/// histogram.record(Duration::from_millis(5));
/// histogram.record(Duration::from_millis(10));
/// histogram.record(Duration::from_millis(100));
///
/// // Get percentiles (in microseconds)
/// let p50 = histogram.p50();
/// let p99 = histogram.p99();
/// assert!(p99 >= p50);
/// ```
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct LatencyHistogram {
    histogram: Mutex<hdrhistogram::Histogram<u64>>,
}

#[cfg(feature = "std")]
impl LatencyHistogram {
    /// Creates a new latency histogram.
    ///
    /// Tracks 1 microsecond to 1 hour with 3 significant figures.
    #[expect(clippy::missing_panics_doc, reason = "readability baseline")]
    pub fn new() -> Self {
        let histogram = hdrhistogram::Histogram::new_with_bounds(1, 3_600_000_000, 3)
            .expect("Failed to create histogram with valid bounds");
        Self {
            histogram: Mutex::new(histogram),
        }
    }

    /// Creates a new histogram tracking `min_us` to `max_us` microseconds with `sigfig` (1-5)
    /// significant figures.
    pub fn with_bounds(min_us: u64, max_us: u64, sigfig: u8) -> Option<Self> {
        hdrhistogram::Histogram::new_with_bounds(min_us, max_us, sigfig)
            .ok()
            .map(|h| Self {
                histogram: Mutex::new(h),
            })
    }

    /// Records a latency value.
    ///
    /// Values above the histogram's max are clamped to it.
    pub fn record(&self, duration: Duration) {
        self.record_micros(duration.as_micros() as u64);
    }

    /// Records a latency value in microseconds.
    pub fn record_micros(&self, micros: u64) {
        let mut hist = self.histogram.lock();
        let max = hist.high();
        let value = micros.min(max);
        if let Err(e) = hist.record(value) {
            // A dropped sample under-counts latency silently, so log it.
            warn!(micros, max, value, error = %e, "latency sample dropped");
        }
    }

    /// Returns the value at the 50th percentile (median) in microseconds.
    pub fn p50(&self) -> u64 {
        self.percentile(50.0)
    }

    /// Returns the value at the 50th percentile (median) in milliseconds.
    pub fn p50_ms(&self) -> f64 {
        self.p50() as f64 / 1000.0
    }

    /// Returns the value at the 95th percentile in microseconds.
    pub fn p95(&self) -> u64 {
        self.percentile(95.0)
    }

    /// Returns the value at the 95th percentile in milliseconds.
    pub fn p95_ms(&self) -> f64 {
        self.p95() as f64 / 1000.0
    }

    /// Returns the value at the 99th percentile in microseconds.
    pub fn p99(&self) -> u64 {
        self.percentile(99.0)
    }

    /// Returns the value at the 99th percentile in milliseconds.
    pub fn p99_ms(&self) -> f64 {
        self.p99() as f64 / 1000.0
    }

    /// Returns the value at the 99.9th percentile in microseconds.
    pub fn p99_9(&self) -> u64 {
        self.percentile(99.9)
    }

    /// Returns the value at the 99.9th percentile in milliseconds.
    pub fn p99_9_ms(&self) -> f64 {
        self.p99_9() as f64 / 1000.0
    }

    /// Returns the value at an arbitrary percentile in microseconds.
    ///
    /// `percentile` is 0.0 to 100.0.
    pub fn percentile(&self, percentile: f64) -> u64 {
        let hist = self.histogram.lock();
        hist.value_at_percentile(percentile)
    }

    /// Returns the mean value in microseconds.
    pub fn mean(&self) -> f64 {
        let hist = self.histogram.lock();
        hist.mean()
    }

    /// Returns the mean value in milliseconds.
    pub fn mean_ms(&self) -> f64 {
        self.mean() / 1000.0
    }

    /// Returns the standard deviation in microseconds.
    pub fn stdev(&self) -> f64 {
        let hist = self.histogram.lock();
        hist.stdev()
    }

    /// Returns the minimum recorded value in microseconds.
    pub fn min(&self) -> u64 {
        let hist = self.histogram.lock();
        hist.min()
    }

    /// Returns the maximum recorded value in microseconds.
    pub fn max(&self) -> u64 {
        let hist = self.histogram.lock();
        hist.max()
    }

    /// Returns the total number of recorded values.
    pub fn count(&self) -> u64 {
        let hist = self.histogram.lock();
        hist.len()
    }

    /// Resets the histogram, clearing all recorded values.
    pub fn reset(&self) {
        let mut hist = self.histogram.lock();
        hist.reset();
    }

    /// Returns a snapshot of percentile values.
    pub fn snapshot(&self) -> PercentileSnapshot {
        let hist = self.histogram.lock();
        PercentileSnapshot {
            count: hist.len(),
            min_us: hist.min(),
            max_us: hist.max(),
            mean_us: hist.mean(),
            p50_us: hist.value_at_percentile(50.0),
            p95_us: hist.value_at_percentile(95.0),
            p99_us: hist.value_at_percentile(99.0),
            p99_9_us: hist.value_at_percentile(99.9),
        }
    }
}

#[cfg(feature = "std")]
impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl Clone for LatencyHistogram {
    fn clone(&self) -> Self {
        let hist = self.histogram.lock();
        Self {
            histogram: Mutex::new(hist.clone()),
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn oversized_sample_is_clamped_and_recorded_without_warning() {
        let hist = LatencyHistogram::new();
        // Far beyond the 1-hour ceiling: clamped and recorded, not dropped, and silently.
        hist.record_micros(u64::MAX);
        assert_eq!(hist.count(), 1);
        assert!(!logs_contain("latency sample dropped"));
    }

    #[traced_test]
    #[test]
    fn ordinary_sample_records_silently() {
        let hist = LatencyHistogram::new();
        hist.record(Duration::from_millis(5));
        assert_eq!(hist.count(), 1);
        assert!(!logs_contain("latency sample dropped"));
    }
}
