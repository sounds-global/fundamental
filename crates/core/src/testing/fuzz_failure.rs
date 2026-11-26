use core::fmt;

/// A property that failed, with everything needed to replay it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzFailure {
    /// The property's name.
    pub target: String,
    /// The root seed the run started from.
    pub root_seed: String,
    /// The batch the failure was found in, counting from zero.
    pub batch: u64,
    /// Why the property failed, as proptest reported it after shrinking.
    pub reason: String,
    /// The minimal failing input, as its `Debug`.
    pub input: String,
}

impl fmt::Display for FuzzFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "fuzz-failure target={} root_seed={} batch={} reason={:?} input={}",
            self.target, self.root_seed, self.batch, self.reason, self.input
        )
    }
}

impl std::error::Error for FuzzFailure {}
