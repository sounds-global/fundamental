use fundamental_core::AnyError;
use fundamental_macros::Error;

use crate::Finding;

/// Why a benchmark, a baseline check or a stability run failed.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BenchError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// Measured instruction counts disagree with the baseline.
    #[error(
        code = "BENCH_001",
        message = "the instruction counts differ from the baseline:\n{listed}\nfix the regression, or run `fun bench --save` and commit the baseline with the reason",
        status = 500
    )]
    Regressed {
        /// Every finding, in bench order.
        findings: Vec<Finding>,
        /// The findings, one per line.
        listed: String,
    },

    /// There is no baseline for this machine's architecture.
    #[error(
        code = "BENCH_002",
        message = "no baseline for {arch} at {path}; run `fun bench --save` to write it",
        status = 404
    )]
    NoBaseline {
        /// The architecture, as Rust names it (`x86_64`, `aarch64`).
        arch: String,
        /// The file that was looked for.
        path: String,
    },

    /// A baseline or a summary file is not what it should hold.
    #[error(
        code = "BENCH_003",
        message = "{path} cannot be read as a baseline: {cause}",
        status = 500
    )]
    Unreadable {
        /// The file, or `<text>` when it was given as text.
        path: String,
        /// What did not fit.
        cause: String,
    },

    /// A file or directory could not be read or written.
    #[error(
        code = "BENCH_004",
        message = "{path} could not be read or written: {cause}",
        status = 500
    )]
    Io {
        /// The file or directory.
        path: String,
        /// The operating system's reason.
        cause: String,
    },

    /// The kernel's status file states no resident memory.
    #[error(
        code = "BENCH_005",
        message = "the process status names no resident memory (VmRSS)",
        status = 500
    )]
    NoResidentSet,

    /// Calls of a run failed: a stability run tolerates none.
    #[error(
        code = "BENCH_006",
        message = "calls of {run} failed:\n{listed}",
        status = 500
    )]
    Errors {
        /// The run.
        run: String,
        /// Each step or phase that had failures, one per line.
        listed: String,
    },

    /// Steps of a run missed the bound they were held to.
    #[error(
        code = "BENCH_007",
        message = "{run} missed {bound}:\n{listed}",
        status = 500
    )]
    Missed {
        /// The run.
        run: String,
        /// The bound (`p99 below 50.00 ms`).
        bound: String,
        /// Each step that missed it, one per line.
        listed: String,
    },

    /// A worker task panicked or was cancelled, so its calls are not in the report.
    #[error(
        code = "BENCH_008",
        message = "a load worker stopped: {cause}",
        status = 500
    )]
    Worker {
        /// Why it stopped.
        cause: String,
    },

    /// A soak's resident memory grew beyond its limit.
    #[error(
        code = "BENCH_009",
        message = "{run}'s resident memory grew {growth_percent}% from the first third of the run to the last, beyond {limit_percent}%",
        status = 500
    )]
    MemoryGrew {
        /// The run.
        run: String,
        /// How much it grew, in percent.
        growth_percent: String,
        /// How much it was allowed to, in percent.
        limit_percent: String,
    },

    /// A soak took too few samples to compare the start with the end.
    #[error(
        code = "BENCH_010",
        message = "{run} took {samples} memory samples, and a verdict needs three; run it longer or sample more often",
        status = 500
    )]
    TooShort {
        /// The run.
        run: String,
        /// How many samples it took.
        samples: usize,
    },

    /// A fault run lost, duplicated or never got through writes.
    #[error(
        code = "BENCH_011",
        message = "{run} did not keep every write: lost {lost}; duplicated {duplicated}; never acknowledged {unacknowledged}",
        status = 500
    )]
    WritesLost {
        /// The run.
        run: String,
        /// The acknowledged writes not found.
        lost: String,
        /// The writes found more than once.
        duplicated: String,
        /// The writes never acknowledged.
        unacknowledged: String,
    },

    /// A fault could not be injected, so the run proves nothing.
    #[error(
        code = "BENCH_012",
        message = "the fault of {run} could not be injected: {cause}",
        status = 500
    )]
    Injection {
        /// The run.
        run: String,
        /// Why.
        cause: String,
    },

    /// A fault run's writes could not be read back.
    #[error(
        code = "BENCH_013",
        message = "the writes of {run} could not be read back: {cause}",
        status = 500
    )]
    ReadBack {
        /// The run.
        run: String,
        /// Why.
        cause: String,
    },

    /// A live run's app or engines could not be brought up.
    #[error(
        code = "BENCH_014",
        message = "the bench app could not {step}: {cause}",
        status = 500
    )]
    Boot {
        /// What it was doing.
        step: String,
        /// Why it failed.
        cause: String,
    },
}

impl BenchError {
    /// The [`Regressed`](Self::Regressed) error for `findings`.
    pub(crate) fn regressed(findings: Vec<Finding>) -> Self {
        let listed = findings
            .iter()
            .map(|finding| format!("  {finding}"))
            .collect::<Vec<_>>()
            .join("\n");
        Self::Regressed { findings, listed }
    }

    /// The [`Io`](Self::Io) error for `path`.
    pub(crate) fn io(path: &std::path::Path, error: &std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            cause: error.to_string(),
        }
    }
}
