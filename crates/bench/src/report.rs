use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

use crate::{BenchError, Step};

/// What a load run measured: one [`Step`] per concurrency, in the order they ran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// What was driven (`pages`, `actions`).
    pub name: String,
    /// Each concurrency's measurement.
    pub steps: Vec<Step>,
}

impl Report {
    /// The report of `name`'s `steps`.
    #[must_use]
    pub fn new(name: impl Into<String>, steps: Vec<Step>) -> Self {
        Self {
            name: name.into(),
            steps,
        }
    }

    /// Each concurrency's measurement, in the order they ran.
    #[must_use]
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Holds every step's 99th percentile below `bound`.
    ///
    /// # Errors
    ///
    /// [`BenchError::Missed`] naming every step that did not.
    pub fn p99_below(&self, bound: Duration) -> Result<(), BenchError> {
        let bound_us = u64::try_from(bound.as_micros()).unwrap_or(u64::MAX);
        let missed: Vec<String> = self
            .steps
            .iter()
            .filter(|step| step.p99_us >= bound_us)
            .map(|step| {
                format!(
                    "concurrency {}: p99 {:.2} ms",
                    step.concurrency,
                    millis(step.p99_us)
                )
            })
            .collect();
        self.judged(missed, format!("p99 below {:.2} ms", millis(bound_us)))
    }

    /// Holds every step to no failed call.
    ///
    /// # Errors
    ///
    /// [`BenchError::Errors`] naming every step that had one.
    pub fn error_free(&self) -> Result<(), BenchError> {
        let failed: Vec<String> = self
            .steps
            .iter()
            .filter(|step| step.errors > 0)
            .map(|step| {
                format!(
                    "concurrency {}: {} of {} calls failed",
                    step.concurrency, step.errors, step.requests
                )
            })
            .collect();
        if failed.is_empty() {
            return Ok(());
        }
        Err(BenchError::Errors {
            run: self.name.clone(),
            listed: failed.join("\n"),
        })
    }

    fn judged(&self, missed: Vec<String>, bound: String) -> Result<(), BenchError> {
        if missed.is_empty() {
            return Ok(());
        }
        Err(BenchError::Missed {
            run: self.name.clone(),
            bound,
            listed: missed.join("\n"),
        })
    }

    /// Writes the report as JSON to `path`, creating its directory.
    ///
    /// # Errors
    ///
    /// [`BenchError::Io`] when it cannot be written.
    #[expect(
        clippy::cognitive_complexity,
        reason = "the span and events expand into branches"
    )]
    #[instrument(skip_all, fields(run = %self.name, path = %path.as_ref().display()), err(Debug))]
    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), BenchError> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|error| BenchError::io(dir, &error))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|error| BenchError::Unreadable {
            path: path.display().to_string(),
            cause: error.to_string(),
        })?;
        fs::write(path, text + "\n").map_err(|error| BenchError::io(path, &error))?;
        info!(steps = self.steps.len(), "bench.report.written");
        Ok(())
    }

    /// The report as a Markdown table: one row per step, latencies in milliseconds.
    #[must_use]
    pub fn table(&self) -> String {
        let mut table = String::from(
            "| concurrency | requests | errors | per second | p50 ms | p95 ms | p99 ms | max ms |\n\
             |---:|---:|---:|---:|---:|---:|---:|---:|\n",
        );
        for step in &self.steps {
            let _ = writeln!(
                table,
                "| {} | {} | {} | {:.0} | {:.2} | {:.2} | {:.2} | {:.2} |",
                step.concurrency,
                step.requests,
                step.errors,
                step.per_second,
                millis(step.p50_us),
                millis(step.p95_us),
                millis(step.p99_us),
                millis(step.max_us),
            );
        }
        table
    }
}

#[expect(clippy::cast_precision_loss, reason = "a latency for a person")]
fn millis(micros: u64) -> f64 {
    micros as f64 / 1_000.0
}
