//! Time bucket unit used to size windows for time-window compaction (TWCS).

use core::fmt::Display;

/// Time window unit for TWCS compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeWindowUnit {
    /// Minutes.
    Minutes,
    /// Hours.
    Hours,
    /// Days.
    Days,
}

impl Display for TimeWindowUnit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TimeWindowUnit::Minutes => write!(f, "minutes"),
            TimeWindowUnit::Hours => write!(f, "hours"),
            TimeWindowUnit::Days => write!(f, "days"),
        }
    }
}
