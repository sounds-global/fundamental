use std::collections::BTreeMap;
use std::env::consts::ARCH;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};

use crate::{BenchError, Checked, Finding, Measured};

/// The growth a bench may show before it is a regression, unless the bench or file names one.
pub const DEFAULT_THRESHOLD: f64 = 0.02;

/// The committed instruction counts the deterministic benches are held to, one file per
/// architecture, as callgrind counts differ between architectures.
///
/// ```rust
/// use fundamental_bench::{Baseline, Measured};
///
/// let baseline = Baseline::from_json(
///     r#"{ "arch": "aarch64", "benches": { "render/page": { "ir": 1000 } } }"#,
/// )?;
/// let measured = Measured::from_iter([("render/page", 1_010)]);
/// assert!(baseline.check(&measured).is_ok());
/// # Ok::<(), fundamental_bench::BenchError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    arch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    toolchain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    threshold: Option<f64>,
    #[serde(default)]
    benches: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    ir: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    threshold: Option<f64>,
}

impl Baseline {
    /// An empty baseline for this machine's architecture.
    #[must_use]
    pub fn new() -> Self {
        Self {
            arch: ARCH.to_owned(),
            toolchain: None,
            threshold: None,
            benches: BTreeMap::new(),
        }
    }

    /// The file in `dir` that holds this machine's architecture's baseline.
    #[must_use]
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join(format!("{ARCH}.json"))
    }

    /// Reads this machine's architecture's baseline from `dir`.
    ///
    /// # Errors
    ///
    /// [`BenchError::NoBaseline`] when `dir` has no file for this architecture,
    /// [`BenchError::Io`] when it cannot be read, and [`BenchError::Unreadable`] when it is not
    /// a baseline or is another architecture's.
    #[expect(
        clippy::cognitive_complexity,
        reason = "the span and events expand into branches"
    )]
    #[instrument(skip_all, fields(dir = %dir.display(), arch = ARCH), err(Debug))]
    pub fn read(dir: &Path) -> Result<Self, BenchError> {
        let path = Self::path_in(dir);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(BenchError::NoBaseline {
                    arch: ARCH.to_owned(),
                    path: path.display().to_string(),
                });
            }
            Err(error) => return Err(BenchError::io(&path, &error)),
        };
        let baseline = Self::parse(&text, &path.display().to_string())?;
        if baseline.arch != ARCH {
            return Err(BenchError::Unreadable {
                path: path.display().to_string(),
                cause: format!(
                    "it is the baseline of {}, and this machine is {ARCH}",
                    baseline.arch
                ),
            });
        }
        Ok(baseline)
    }

    /// Reads a baseline from its JSON text.
    ///
    /// # Errors
    ///
    /// [`BenchError::Unreadable`] when `text` is not a baseline.
    pub fn from_json(text: &str) -> Result<Self, BenchError> {
        Self::parse(text, "<text>")
    }

    fn parse(text: &str, path: &str) -> Result<Self, BenchError> {
        serde_json::from_str(text).map_err(|error| BenchError::Unreadable {
            path: path.to_owned(),
            cause: error.to_string(),
        })
    }

    /// The architecture whose counts these are.
    #[must_use]
    pub fn arch(&self) -> &str {
        &self.arch
    }

    /// The toolchain the counts were measured with, when the file says.
    #[must_use]
    pub fn toolchain(&self) -> Option<&str> {
        self.toolchain.as_deref()
    }

    /// This baseline, recording `toolchain` as the one it was measured with.
    #[must_use]
    pub fn with_toolchain(mut self, toolchain: impl Into<String>) -> Self {
        self.toolchain = Some(toolchain.into());
        self
    }

    /// Checks `measured` against this baseline.
    ///
    /// # Errors
    ///
    /// [`BenchError::Regressed`] naming every [`Finding`] at once.
    #[expect(
        clippy::cognitive_complexity,
        reason = "the span and events expand into branches"
    )]
    #[instrument(skip_all, fields(arch = %self.arch, benches = self.benches.len(), measured = measured.len()), err(Debug))]
    pub fn check(&self, measured: &Measured) -> Result<Checked, BenchError> {
        let default = self.threshold.unwrap_or(DEFAULT_THRESHOLD);
        let mut findings = Vec::new();
        let mut checked = Checked::default();
        for (bench, entry) in &self.benches {
            let Some(now) = measured.get(bench) else {
                findings.push(Finding::NotMeasured {
                    bench: bench.clone(),
                });
                continue;
            };
            checked.compared += 1;
            let limit = entry.threshold.unwrap_or(default);
            #[expect(
                clippy::cast_precision_loss,
                reason = "counts far below 2^52, compared as a ratio"
            )]
            let ratio = now as f64 / entry.ir.max(1) as f64;
            if ratio > 1.0 + limit {
                findings.push(Finding::Regressed {
                    bench: bench.clone(),
                    was: entry.ir,
                    now,
                    limit,
                });
            } else if ratio < 1.0 - limit {
                info!(bench, was = entry.ir, now, "bench.baseline.improved");
                checked.improved.push(bench.clone());
            }
        }
        for (bench, _) in measured.iter() {
            if !self.benches.contains_key(bench) {
                findings.push(Finding::NotInBaseline {
                    bench: bench.to_owned(),
                });
            }
        }
        if findings.is_empty() {
            info!(
                compared = checked.compared,
                improved = checked.improved.len(),
                "bench.baseline.passed"
            );
            Ok(checked)
        } else {
            findings.sort_by(|a, b| a.bench().cmp(b.bench()));
            warn!(findings = findings.len(), "bench.baseline.differs");
            Err(BenchError::regressed(findings))
        }
    }

    /// This baseline with `measured`'s counts, keeping each bench's own threshold.
    #[must_use]
    pub fn saved(&self, measured: &Measured) -> Self {
        let benches = measured
            .iter()
            .map(|(bench, ir)| {
                let threshold = self.benches.get(bench).and_then(|entry| entry.threshold);
                (bench.to_owned(), Entry { ir, threshold })
            })
            .collect();
        Self {
            benches,
            ..self.clone()
        }
    }

    /// The baseline as the JSON text its file holds.
    #[must_use]
    pub fn to_json(&self) -> String {
        match serde_json::to_string_pretty(self) {
            Ok(text) => text + "\n",
            Err(error) => {
                warn!(%error, "bench.baseline.unserialisable");
                String::new()
            }
        }
    }

    /// Writes the baseline to its architecture's file in `dir`, creating `dir`.
    ///
    /// # Errors
    ///
    /// [`BenchError::Io`] when the file cannot be written.
    #[expect(
        clippy::cognitive_complexity,
        reason = "the span and events expand into branches"
    )]
    #[instrument(skip_all, fields(dir = %dir.display(), arch = %self.arch), err(Debug))]
    pub fn write(&self, dir: &Path) -> Result<PathBuf, BenchError> {
        fs::create_dir_all(dir).map_err(|error| BenchError::io(dir, &error))?;
        let path = dir.join(format!("{}.json", self.arch));
        fs::write(&path, self.to_json()).map_err(|error| BenchError::io(&path, &error))?;
        info!(path = %path.display(), benches = self.benches.len(), "bench.baseline.written");
        Ok(path)
    }
}

impl Default for Baseline {
    fn default() -> Self {
        Self::new()
    }
}
