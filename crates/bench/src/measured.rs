use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;
use tracing::{debug, instrument};

use crate::BenchError;

/// The file gungraun writes per bench with `--save-summary=json`.
const SUMMARY: &str = "summary.json";

/// The instruction counts (callgrind's `Ir`) one run produced, by bench.
///
/// A bench is named `<package>/<bench file>/<group>/<function>.<id>`, its summary's directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Measured {
    counts: BTreeMap<String, u64>,
}

impl Measured {
    /// No counts.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every count gungraun wrote under `dir` (its output directory, `target/gungraun`).
    ///
    /// # Errors
    ///
    /// [`BenchError::Io`] when a directory or file cannot be read, and
    /// [`BenchError::Unreadable`] when a summary has no callgrind instruction count.
    #[expect(
        clippy::cognitive_complexity,
        reason = "the span and events expand into branches"
    )]
    #[instrument(skip_all, fields(dir = %dir.display()), err(Debug))]
    pub fn read_summaries(dir: &Path) -> Result<Self, BenchError> {
        let mut measured = Self::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(at) = pending.pop() {
            let entries = fs::read_dir(&at).map_err(|error| BenchError::io(&at, &error))?;
            for entry in entries {
                let path = entry.map_err(|error| BenchError::io(&at, &error))?.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.file_name().is_some_and(|name| name == SUMMARY) {
                    let bench = bench_name(dir, &path);
                    let instructions = instructions_in(&path)?;
                    debug!(%bench, instructions, "bench.summary.read");
                    measured.insert(bench, instructions);
                }
            }
        }
        Ok(measured)
    }

    /// Records `bench`'s count, replacing an earlier one.
    pub fn insert(&mut self, bench: impl Into<String>, instructions: u64) {
        self.counts.insert(bench.into(), instructions);
    }

    /// `bench`'s count, when it was measured.
    #[must_use]
    pub fn get(&self, bench: &str) -> Option<u64> {
        self.counts.get(bench).copied()
    }

    /// Every bench and its count, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, u64)> {
        self.counts
            .iter()
            .map(|(bench, count)| (bench.as_str(), *count))
    }

    /// How many benches were measured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.counts.len()
    }

    /// Whether nothing was measured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }
}

impl<S: Into<String>> FromIterator<(S, u64)> for Measured {
    fn from_iter<I: IntoIterator<Item = (S, u64)>>(iter: I) -> Self {
        let mut measured = Self::new();
        for (bench, count) in iter {
            measured.insert(bench, count);
        }
        measured
    }
}

fn bench_name(root: &Path, summary: &Path) -> String {
    summary
        .parent()
        .and_then(|dir| dir.strip_prefix(root).ok())
        .map(|dir| {
            dir.components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}

fn instructions_in(path: &Path) -> Result<u64, BenchError> {
    let unreadable = |cause: &str| BenchError::Unreadable {
        path: path.display().to_string(),
        cause: cause.to_owned(),
    };
    let text = fs::read_to_string(path).map_err(|error| BenchError::io(path, &error))?;
    let summary: Value =
        serde_json::from_str(&text).map_err(|error| unreadable(&error.to_string()))?;
    summary["profiles"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|profile| profile["tool"] == "Callgrind")
        .and_then(|profile| profile["data"]["total"]["metrics"]["Ir"]["values"]["new"].as_u64())
        .ok_or_else(|| {
            unreadable("it has no callgrind instruction count (profiles[].data.total.metrics.Ir)")
        })
}
