use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::latencies::Latencies;

/// One concurrency of a load run, measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// How many calls were in flight at once.
    pub concurrency: usize,
    /// How long the step was measured, in seconds.
    pub seconds: f64,
    /// How many calls finished, failed ones included.
    pub requests: u64,
    /// How many of them failed.
    pub errors: u64,
    /// Finished calls per second.
    pub per_second: f64,
    /// The median latency, in microseconds.
    pub p50_us: u64,
    /// The 95th percentile latency, in microseconds.
    pub p95_us: u64,
    /// The 99th percentile latency, in microseconds.
    pub p99_us: u64,
    /// The longest call, in microseconds.
    pub max_us: u64,
}

impl Step {
    pub(crate) fn measured(
        concurrency: usize,
        took: Duration,
        latencies: &Latencies,
        errors: u64,
    ) -> Self {
        let seconds = took.as_secs_f64();
        let requests = latencies.len();
        #[expect(clippy::cast_precision_loss, reason = "a rate for a person")]
        let per_second = if seconds > 0.0 {
            requests as f64 / seconds
        } else {
            0.0
        };
        Self {
            concurrency,
            seconds,
            requests,
            errors,
            per_second,
            p50_us: latencies.quantile_us(0.50),
            p95_us: latencies.quantile_us(0.95),
            p99_us: latencies.quantile_us(0.99),
            max_us: latencies.max_us(),
        }
    }
}
