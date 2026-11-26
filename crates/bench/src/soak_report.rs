use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{BenchError, Step};

/// What a soak measured: resident memory over time and the load it carried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoakReport {
    /// What was driven.
    pub name: String,
    /// The growth the last third's memory may show over the first third's.
    pub limit: f64,
    /// Each sample: seconds since the start, and resident bytes.
    pub samples: Vec<(f64, u64)>,
    /// The whole run's calls, as one step.
    pub load: Step,
}

impl SoakReport {
    /// How much the last third's average memory grew over the first third's, as a fraction;
    /// none with fewer than three samples.
    #[must_use]
    pub fn growth(&self) -> Option<f64> {
        growth_of(&self.samples)
    }

    /// The verdict: no failed call, and memory that grew less than the limit.
    ///
    /// # Errors
    ///
    /// [`BenchError::Errors`] when a call failed, [`BenchError::TooShort`] with fewer than
    /// three samples, and [`BenchError::MemoryGrew`] when memory grew beyond the limit.
    pub fn judge(&self) -> Result<(), BenchError> {
        if self.load.errors > 0 {
            return Err(BenchError::Errors {
                run: self.name.clone(),
                listed: format!(
                    "{} of {} calls failed",
                    self.load.errors, self.load.requests
                ),
            });
        }
        let growth = self.growth().ok_or(BenchError::TooShort {
            run: self.name.clone(),
            samples: self.samples.len(),
        })?;
        if growth > self.limit {
            return Err(BenchError::MemoryGrew {
                run: self.name.clone(),
                growth_percent: format!("{:.1}", growth * 100.0),
                limit_percent: format!("{:.1}", self.limit * 100.0),
            });
        }
        info!(run = %self.name, growth, limit = self.limit, "bench.soak.held");
        Ok(())
    }

    /// Writes the report as JSON to `path`, creating its directory.
    ///
    /// # Errors
    ///
    /// [`BenchError::Io`] when it cannot be written.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), BenchError> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|error| BenchError::io(dir, &error))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|error| BenchError::Unreadable {
            path: path.display().to_string(),
            cause: error.to_string(),
        })?;
        fs::write(path, text + "\n").map_err(|error| BenchError::io(path, &error))
    }
}

pub(crate) fn growth_of(samples: &[(f64, u64)]) -> Option<f64> {
    let third = samples.len() / 3;
    if third == 0 {
        return None;
    }
    let first = average(&samples[..third]);
    let last = average(&samples[samples.len() - third..]);
    (first > 0.0).then(|| last / first - 1.0)
}

#[expect(clippy::cast_precision_loss, reason = "an average of byte counts")]
fn average(samples: &[(f64, u64)]) -> f64 {
    samples.iter().map(|(_, bytes)| *bytes as f64).sum::<f64>() / samples.len() as f64
}
