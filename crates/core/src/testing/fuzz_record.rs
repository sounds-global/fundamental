use core::fmt;
use std::time::Duration;

/// What a property run did: the line a spec records as evidence that it ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzRecord {
    /// The property's name.
    pub target: String,
    /// Cases run in total.
    pub cases: u64,
    /// Batches run, each from its own derived seed.
    pub batches: u64,
    /// The root seed, in hex, to replay the run with `FUNDAMENTAL_FUZZ_SEED`.
    pub root_seed: String,
    /// Wall-clock time spent.
    pub elapsed: Duration,
}

impl fmt::Display for FuzzRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "fuzz-record target={} cases={} batches={} root_seed={} elapsed_s={}",
            self.target,
            self.cases,
            self.batches,
            self.root_seed,
            self.elapsed.as_secs()
        )
    }
}
