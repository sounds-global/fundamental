use std::time::Duration;

use hdrhistogram::Histogram;
use tracing::warn;

/// The longest latency recorded exactly, an hour; a longer call is recorded as an hour.
const HIGHEST_US: u64 = 3_600_000_000;

/// Every call's latency of one step, in microseconds, to three significant figures.
#[derive(Debug, Clone)]
pub(crate) struct Latencies {
    histogram: Histogram<u64>,
}

impl Latencies {
    pub(crate) fn new() -> Self {
        let histogram = Histogram::new_with_bounds(1, HIGHEST_US, 3)
            .unwrap_or_else(|_| Histogram::new(3).unwrap_or_else(|_| unreachable_histogram()));
        Self { histogram }
    }

    pub(crate) fn record(&mut self, took: Duration) {
        let micros = u64::try_from(took.as_micros())
            .unwrap_or(HIGHEST_US)
            .clamp(1, HIGHEST_US);
        if let Err(error) = self.histogram.record(micros) {
            warn!(micros, %error, "bench.latency.unrecorded");
        }
    }

    pub(crate) fn add(&mut self, other: &Self) {
        if let Err(error) = self.histogram.add(&other.histogram) {
            warn!(%error, "bench.latency.unmerged");
        }
    }

    pub(crate) fn len(&self) -> u64 {
        self.histogram.len()
    }

    /// The latency `quantile` (`0.99`) of the calls lie at or below, in microseconds.
    pub(crate) fn quantile_us(&self, quantile: f64) -> u64 {
        self.histogram.value_at_quantile(quantile)
    }

    /// The longest call, in microseconds.
    pub(crate) fn max_us(&self) -> u64 {
        self.histogram.max()
    }
}

#[expect(
    clippy::panic,
    reason = "a programmer-error invariant: three significant figures is always valid"
)]
fn unreachable_histogram() -> Histogram<u64> {
    panic!("a histogram of three significant figures is always valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_percentiles_of_a_hundred_calls_are_their_ranks() {
        let mut latencies = Latencies::new();
        for millis in 1..=100 {
            latencies.record(Duration::from_millis(millis));
        }
        assert_eq!(latencies.len(), 100);
        assert_eq!(latencies.quantile_us(0.50) / 1_000, 50);
        assert_eq!(latencies.quantile_us(0.95) / 1_000, 95);
        assert_eq!(latencies.quantile_us(0.99) / 1_000, 99);
        assert_eq!(latencies.max_us() / 1_000, 100);
    }

    #[test]
    fn two_workers_add_up() {
        let mut one = Latencies::new();
        let mut two = Latencies::new();
        one.record(Duration::from_millis(1));
        two.record(Duration::from_millis(3));
        one.add(&two);
        assert_eq!(one.len(), 2);
        assert_eq!(one.max_us() / 1_000, 3);
    }

    #[test]
    fn a_zero_latency_is_recorded_as_one_microsecond() {
        let mut latencies = Latencies::new();
        latencies.record(Duration::ZERO);
        assert_eq!(latencies.max_us(), 1);
    }
}
