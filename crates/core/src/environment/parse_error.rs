//! Error type for failed [`Environment`](super::Environment) parsing.

#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;

/// Error returned when parsing an invalid environment string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseEnvironmentError {
    /// The invalid input string.
    input: String,
}

impl ParseEnvironmentError {
    /// Creates a new parse error.
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
        }
    }

    /// Returns the invalid input string.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }
}

impl fmt::Display for ParseEnvironmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid environment '{}': expected one of 'development', 'test', 'staging', 'production'",
            self.input
        )
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ParseEnvironmentError {}
