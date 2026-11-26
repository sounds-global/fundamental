use core::fmt;

/// A fuzz budget setting that is not in its one spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FuzzBudgetError {
    /// `FUNDAMENTAL_FUZZ_FOR` is not a positive whole number of seconds followed by `s`.
    Duration(String),
    /// `FUNDAMENTAL_FUZZ_SEED` is not 64 hex characters.
    Seed(String),
}

impl fmt::Display for FuzzBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duration(text) => write!(
                formatter,
                "FUNDAMENTAL_FUZZ_FOR={text:?} is not a duration like `3600s`"
            ),
            Self::Seed(text) => write!(
                formatter,
                "FUNDAMENTAL_FUZZ_SEED={text:?} is not 64 hex characters"
            ),
        }
    }
}

impl std::error::Error for FuzzBudgetError {}
