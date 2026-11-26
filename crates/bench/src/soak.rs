use std::fmt::Display;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::{Instant, MissedTickBehavior, interval};
use tracing::{info, instrument};

use crate::load::drive;
use crate::{BenchError, Rss, SoakReport, Step};

/// A call held at one concurrency for a long time while resident memory is sampled.
///
/// The verdict compares the last third's average memory with the first third's, so warm-up
/// growth is judged against the steady state.
///
/// ```rust
/// use std::time::Duration;
/// use fundamental_bench::Soak;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), fundamental_bench::BenchError> {
/// let report = Soak::new("noop")
///     .concurrency(4)
///     .for_duration(Duration::from_millis(60))
///     .sample_every(Duration::from_millis(10))
///     .rss_growth_below(0.10)
///     .run(|| async { Ok::<(), String>(()) })
///     .await?;
/// assert!(report.samples.len() >= 3);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub struct Soak {
    name: String,
    concurrency: usize,
    duration: Duration,
    sample_every: Duration,
    limit: f64,
}

impl Soak {
    /// A soak of `name`: by default 32 calls at once for ten minutes, memory sampled every ten
    /// seconds and allowed to grow by 10%.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            concurrency: 32,
            duration: Duration::from_secs(600),
            sample_every: Duration::from_secs(10),
            limit: 0.10,
        }
    }

    /// How many calls are in flight at once; a zero is run as one.
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    /// How long the soak runs.
    pub fn for_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// How often resident memory is sampled; a zero is sampled every millisecond.
    pub fn sample_every(mut self, every: Duration) -> Self {
        self.sample_every = every.max(Duration::from_millis(1));
        self
    }

    /// The growth the last third's memory may show over the first third's, as a fraction.
    pub fn rss_growth_below(mut self, limit: f64) -> Self {
        self.limit = limit;
        self
    }

    /// Runs the soak and reports it; [`SoakReport::judge`] gives the verdict.
    ///
    /// # Errors
    ///
    /// [`BenchError::Worker`] when a worker stopped, and the error of [`Rss::now`] when memory
    /// cannot be read.
    #[instrument(skip_all, fields(run = %self.name, concurrency = self.concurrency, secs = self.duration.as_secs_f64()), err(Debug))]
    pub async fn run<F, Fut, E>(self, call: F) -> Result<SoakReport, BenchError>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), E>> + Send + 'static,
        E: Display + Send + 'static,
    {
        let call = Arc::new(call);
        let started = Instant::now();
        let concurrency = self.concurrency;
        let duration = self.duration;
        let load = tokio::spawn(async move { drive(&call, concurrency, duration).await });
        let samples = sample(started, self.duration, self.sample_every).await?;
        let (latencies, errors) = load.await.map_err(|error| BenchError::Worker {
            cause: error.to_string(),
        })??;
        let load = Step::measured(self.concurrency, started.elapsed(), &latencies, errors);
        info!(run = %self.name, samples = samples.len(), requests = load.requests, errors, "bench.soak.done");
        Ok(SoakReport {
            name: self.name,
            limit: self.limit,
            samples,
            load,
        })
    }
}

async fn sample(
    started: Instant,
    duration: Duration,
    every: Duration,
) -> Result<Vec<(f64, u64)>, BenchError> {
    let mut ticks = interval(every);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut samples = Vec::new();
    loop {
        ticks.tick().await;
        let at = started.elapsed();
        let rss = Rss::now()?;
        samples.push((at.as_secs_f64(), rss.bytes()));
        if at >= duration {
            return Ok(samples);
        }
        if samples.len() % 30 == 0 {
            log_trend(&samples);
        }
    }
}

fn log_trend(samples: &[(f64, u64)]) {
    let growth = crate::soak_report::growth_of(samples);
    info!(?growth, samples = samples.len(), "bench.soak.trend");
}
