use std::fmt::Display;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinSet;
use tokio::time::Instant;
use tracing::{info, instrument, warn};

use crate::latencies::Latencies;
use crate::{BenchError, Report, Step};

/// How many failed calls of one step are logged with their reason; the rest are counted.
const LOGGED_ERRORS: u64 = 5;

/// A call driven at rising concurrency: for each step, that many workers call it back to back
/// for a fixed time after an unmeasured warm-up.
///
/// A closed loop: a worker calls again when its last call answered, so a slow server lowers
/// throughput instead of piling up calls.
///
/// ```rust
/// use std::time::Duration;
/// use fundamental_bench::Load;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), fundamental_bench::BenchError> {
/// let report = Load::new("noop")
///     .concurrency([1, 8])
///     .each_for(Duration::from_millis(20))
///     .warm_for(Duration::ZERO)
///     .run(|| async { Ok::<(), String>(()) })
///     .await?;
/// assert_eq!(report.steps().len(), 2);
/// report.error_free()
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub struct Load {
    name: String,
    concurrency: Vec<usize>,
    each_for: Duration,
    warm_for: Duration,
}

impl Load {
    /// A load run of `name`: by default at concurrency 1, 8, 32 and 128, each for 20 seconds
    /// after a one-second warm-up.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            concurrency: vec![1, 8, 32, 128],
            each_for: Duration::from_secs(20),
            warm_for: Duration::from_secs(1),
        }
    }

    /// The concurrency of each step, in the order they run; a zero is run as one.
    pub fn concurrency(mut self, steps: impl IntoIterator<Item = usize>) -> Self {
        self.concurrency = steps.into_iter().map(|n| n.max(1)).collect();
        self
    }

    /// How long each step is measured.
    pub fn each_for(mut self, duration: Duration) -> Self {
        self.each_for = duration;
        self
    }

    /// How long each step runs before it is measured.
    pub fn warm_for(mut self, duration: Duration) -> Self {
        self.warm_for = duration;
        self
    }

    /// Runs every step, calling `call` from each worker; a failed call counts in `errors`.
    ///
    /// # Errors
    ///
    /// [`BenchError::Worker`] when a worker task panicked or was cancelled.
    #[instrument(skip_all, fields(run = %self.name, steps = ?self.concurrency, each_secs = self.each_for.as_secs_f64()), err(Debug))]
    pub async fn run<F, Fut, E>(self, call: F) -> Result<Report, BenchError>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), E>> + Send + 'static,
        E: Display + Send + 'static,
    {
        let call = Arc::new(call);
        let mut steps = Vec::with_capacity(self.concurrency.len());
        for &concurrency in &self.concurrency {
            if !self.warm_for.is_zero() {
                drive(&call, concurrency, self.warm_for).await?;
            }
            let started = Instant::now();
            let (latencies, errors) = drive(&call, concurrency, self.each_for).await?;
            let step = Step::measured(concurrency, started.elapsed(), &latencies, errors);
            info!(
                run = %self.name,
                concurrency,
                requests = step.requests,
                errors = step.errors,
                per_second = step.per_second,
                p99_us = step.p99_us,
                "bench.load.step"
            );
            steps.push(step);
        }
        Ok(Report::new(self.name, steps))
    }
}

/// Runs `concurrency` workers calling `call` back to back for `duration`.
pub(crate) async fn drive<F, Fut, E>(
    call: &Arc<F>,
    concurrency: usize,
    duration: Duration,
) -> Result<(Latencies, u64), BenchError>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), E>> + Send + 'static,
    E: Display + Send + 'static,
{
    let deadline = Instant::now() + duration;
    let mut workers = JoinSet::new();
    for worker in 0..concurrency {
        let call = Arc::clone(call);
        workers.spawn(work(call, worker, deadline));
    }
    let mut all = Latencies::new();
    let mut errors = 0;
    while let Some(joined) = workers.join_next().await {
        let (latencies, failed) = joined.map_err(|error| BenchError::Worker {
            cause: error.to_string(),
        })?;
        all.add(&latencies);
        errors += failed;
    }
    Ok((all, errors))
}

async fn work<F, Fut, E>(call: Arc<F>, worker: usize, deadline: Instant) -> (Latencies, u64)
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), E>> + Send + 'static,
    E: Display + Send + 'static,
{
    let mut latencies = Latencies::new();
    let mut errors = 0_u64;
    while Instant::now() < deadline {
        let started = Instant::now();
        let answered = call().await;
        latencies.record(started.elapsed());
        // Yields once the cooperative budget is spent, so a call that never waits cannot starve
        // the other workers and a soak's sampler.
        tokio::task::consume_budget().await;
        let Err(error) = answered else { continue };
        errors += 1;
        if errors <= LOGGED_ERRORS {
            warn!(worker, %error, "bench.load.call_failed");
        }
    }
    (latencies, errors)
}
