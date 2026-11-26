//! Error type for failed [`EnvironmentPosture`](super::EnvironmentPosture) parsing.

#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;

use super::EnvironmentPosture;

/// Error returned when a posture is neither `deployed` nor `development`, exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePostureError {
    input: String,
}

impl ParsePostureError {
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

impl fmt::Display for ParsePostureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\"{}\" is not a posture: write {}",
            self.input,
            EnvironmentPosture::ACCEPTED
        )
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ParsePostureError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_names_the_input_and_what_is_accepted() {
        let err = ParsePostureError::new("deploy");
        assert_eq!(err.input(), "deploy");
        assert_eq!(
            err.to_string(),
            "\"deploy\" is not a posture: write \"deployed\" or \"development\""
        );
    }
}
