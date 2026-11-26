//! Percentile latency tracking via HDR histograms (`std` only).

mod latency_histogram;
mod snapshot;

pub use latency_histogram::LatencyHistogram;
pub use snapshot::PercentileSnapshot;
