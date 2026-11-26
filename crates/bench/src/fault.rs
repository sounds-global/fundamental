use std::fmt::Display;
use std::future::Future;
use std::time::Duration;

use tokio::time::{Instant, sleep};
use tracing::{info, instrument, warn};

use crate::{BenchError, FaultReport};

/// Numbered writes made while a fault is injected, each retried until acknowledged, then read
/// back: every acknowledged write must be found exactly once.
///
/// A write whose answer was lost is retried with the same number, so a non-idempotent write
/// path shows up as a duplicate.
///
/// ```rust
/// use std::time::Duration;
/// use std::sync::{Arc, Mutex};
/// use fundamental_bench::Fault;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), fundamental_bench::BenchError> {
/// let rows = Arc::new(Mutex::new(Vec::new()));
/// let (writer, reader) = (rows.clone(), rows.clone());
/// let report = Fault::new("nothing")
///     .writes(10)
///     .pace(Duration::ZERO)
///     .inject_after(Duration::ZERO)
///     .run(
///         move |n| {
///             let writer = writer.clone();
///             async move { writer.lock().map(|mut rows| rows.push(n)).map_err(|_| "poisoned") }
///         },
///         async { Ok::<(), String>(()) },
///         move || async move { reader.lock().map(|rows| rows.clone()).map_err(|_| "poisoned") },
///     )
///     .await?;
/// report.judge()
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub struct Fault {
    name: String,
    writes: u64,
    pace: Duration,
    inject_after: Duration,
    retry_every: Duration,
    give_up_after: Duration,
}

impl Fault {
    /// A fault run of `name`: by default 500 writes ten milliseconds apart, the fault injected
    /// after two seconds, a refused write retried every 100 ms for up to two minutes.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            writes: 500,
            pace: Duration::from_millis(10),
            inject_after: Duration::from_secs(2),
            retry_every: Duration::from_millis(100),
            give_up_after: Duration::from_secs(120),
        }
    }

    /// How many numbered writes are made, numbered from zero.
    pub fn writes(mut self, writes: u64) -> Self {
        self.writes = writes;
        self
    }

    /// The pause between one acknowledged write and the next.
    pub fn pace(mut self, pace: Duration) -> Self {
        self.pace = pace;
        self
    }

    /// How long after the first write the fault is injected.
    pub fn inject_after(mut self, after: Duration) -> Self {
        self.inject_after = after;
        self
    }

    /// The pause before a refused write is tried again.
    pub fn retry_every(mut self, every: Duration) -> Self {
        self.retry_every = every;
        self
    }

    /// How long one write is retried before the run gives up on it.
    pub fn give_up_after(mut self, after: Duration) -> Self {
        self.give_up_after = after;
        self
    }

    /// Makes the writes with `write` while `inject` runs, then reads them back with
    /// `read_back`; `inject` should return once the fault is over.
    ///
    /// # Errors
    ///
    /// [`BenchError::Injection`] when `inject` failed, so the fault may not have happened, and
    /// [`BenchError::ReadBack`] when the writes cannot be read back.
    #[instrument(skip_all, fields(run = %self.name, writes = self.writes), err(Debug))]
    pub async fn run<W, WFut, WE, I, IE, R, RFut, RE>(
        self,
        write: W,
        inject: I,
        read_back: R,
    ) -> Result<FaultReport, BenchError>
    where
        W: Fn(u64) -> WFut,
        WFut: Future<Output = Result<(), WE>>,
        WE: Display,
        I: Future<Output = Result<(), IE>>,
        IE: Display,
        R: FnOnce() -> RFut,
        RFut: Future<Output = Result<Vec<u64>, RE>>,
        RE: Display,
    {
        let started = Instant::now();
        let injected = async {
            sleep(self.inject_after).await;
            info!(run = %self.name, "bench.fault.injecting");
            let done = inject.await;
            info!(run = %self.name, ok = done.is_ok(), "bench.fault.injected");
            done
        };
        let (written, injected) = tokio::join!(self.write_all(&write, started), injected);
        injected.map_err(|error| BenchError::Injection {
            run: self.name.clone(),
            cause: error.to_string(),
        })?;
        let found = read_back().await.map_err(|error| BenchError::ReadBack {
            run: self.name.clone(),
            cause: error.to_string(),
        })?;
        let mut report = FaultReport::of(&self.name, self.writes, &written.acknowledged, &found);
        report.failed_attempts = written.failed_attempts;
        report.outage_secs = written.outage.map(|outage| outage.as_secs_f64());
        info!(
            run = %self.name,
            acknowledged = report.acknowledged,
            failed_attempts = report.failed_attempts,
            lost = report.lost.len(),
            duplicated = report.duplicated.len(),
            outage_secs = ?report.outage_secs,
            "bench.fault.done"
        );
        Ok(report)
    }

    async fn write_all<W, WFut, WE>(&self, write: &W, started: Instant) -> Written
    where
        W: Fn(u64) -> WFut,
        WFut: Future<Output = Result<(), WE>>,
        WE: Display,
    {
        let mut written = Written::default();
        for n in 0..self.writes {
            self.write_one(write, n, &mut written).await;
            if let Some(outage) = written.recovered() {
                info!(run = %self.name, n, outage_secs = outage.as_secs_f64(), at_secs = started.elapsed().as_secs_f64(), "bench.fault.recovered");
            }
            sleep(self.pace).await;
        }
        written
    }

    #[expect(
        clippy::cognitive_complexity,
        reason = "the two events expand into branches"
    )]
    async fn write_one<W, WFut, WE>(&self, write: &W, n: u64, written: &mut Written)
    where
        W: Fn(u64) -> WFut,
        WFut: Future<Output = Result<(), WE>>,
        WE: Display,
    {
        let since = Instant::now();
        loop {
            let Err(error) = write(n).await else {
                written.acknowledged.push(n);
                return;
            };
            if written.refused() {
                warn!(run = %self.name, n, failed_attempts = written.failed_attempts, %error, "bench.fault.write_refused");
            }
            if since.elapsed() >= self.give_up_after {
                warn!(run = %self.name, n, "bench.fault.gave_up");
                return;
            }
            sleep(self.retry_every).await;
        }
    }
}

#[derive(Debug, Default)]
struct Written {
    acknowledged: Vec<u64>,
    failed_attempts: u64,
    /// From the first refused write to the next acknowledged one, summed over every outage.
    outage: Option<Duration>,
    failing_since: Option<Instant>,
}

impl Written {
    /// Counts a refused attempt; whether it is worth logging (the first, then every fiftieth).
    fn refused(&mut self) -> bool {
        self.failed_attempts += 1;
        self.failing_since.get_or_insert_with(Instant::now);
        self.failed_attempts == 1 || self.failed_attempts.is_multiple_of(50)
    }

    /// Ends the current outage after an acknowledged write, and returns how long it was.
    fn recovered(&mut self) -> Option<Duration> {
        let outage = self.failing_since.take()?.elapsed();
        self.outage = Some(self.outage.unwrap_or_default() + outage);
        Some(outage)
    }
}
