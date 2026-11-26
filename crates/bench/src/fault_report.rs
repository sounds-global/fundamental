use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::BenchError;

/// What a fault run wrote and found again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FaultReport {
    /// The fault.
    pub name: String,
    /// How many numbered writes were made.
    pub writes: u64,
    /// How many were acknowledged.
    pub acknowledged: u64,
    /// How many attempts were refused along the way.
    pub failed_attempts: u64,
    /// The outage as the writer saw it, in seconds; none when no write was refused.
    pub outage_secs: Option<f64>,
    /// Writes the run gave up on: never acknowledged.
    pub unacknowledged: Vec<u64>,
    /// Acknowledged writes that were not found.
    pub lost: Vec<u64>,
    /// Writes found more than once.
    pub duplicated: Vec<u64>,
}

impl FaultReport {
    /// The report of `writes` numbered writes, the `acknowledged` ones, and what was `found`.
    #[must_use]
    pub fn of(name: &str, writes: u64, acknowledged: &[u64], found: &[u64]) -> Self {
        let mut counts: BTreeMap<u64, u32> = BTreeMap::new();
        for n in found {
            *counts.entry(*n).or_default() += 1;
        }
        let acked: std::collections::BTreeSet<u64> = acknowledged.iter().copied().collect();
        Self {
            name: name.to_owned(),
            writes,
            acknowledged: acked.len() as u64,
            failed_attempts: 0,
            outage_secs: None,
            unacknowledged: (0..writes).filter(|n| !acked.contains(n)).collect(),
            lost: acked
                .iter()
                .filter(|n| !counts.contains_key(n))
                .copied()
                .collect(),
            duplicated: counts
                .iter()
                .filter(|(_, times)| **times > 1)
                .map(|(n, _)| *n)
                .collect(),
        }
    }

    /// The verdict: every write acknowledged, and each found exactly once.
    ///
    /// # Errors
    ///
    /// [`BenchError::WritesLost`] naming the writes lost, duplicated and never acknowledged.
    pub fn judge(&self) -> Result<(), BenchError> {
        if self.lost.is_empty() && self.duplicated.is_empty() && self.unacknowledged.is_empty() {
            return Ok(());
        }
        Err(BenchError::WritesLost {
            run: self.name.clone(),
            lost: listed(&self.lost),
            duplicated: listed(&self.duplicated),
            unacknowledged: listed(&self.unacknowledged),
        })
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

/// Up to twenty numbers and how many more.
fn listed(numbers: &[u64]) -> String {
    if numbers.is_empty() {
        return "none".to_owned();
    }
    let shown: Vec<String> = numbers.iter().take(20).map(u64::to_string).collect();
    let more = numbers.len().saturating_sub(20);
    if more == 0 {
        shown.join(", ")
    } else {
        format!("{} and {more} more", shown.join(", "))
    }
}
