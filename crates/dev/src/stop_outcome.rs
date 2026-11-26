//! [`StopOutcome`]: how a supervised app's process ended.

use std::process::ExitStatus;
use std::time::Duration;

/// What happened when the supervisor asked an app to stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopOutcome {
    /// The process had already exited on its own before it was asked to stop.
    AlreadyExited(ExitStatus),
    /// `SIGTERM` was honoured and the process drained within the grace period, taking this long.
    Drained(Duration),
    /// The grace period elapsed and the process was killed.
    Killed(Duration),
}

impl StopOutcome {
    /// Whether the process shut down cleanly — either on its own or by draining.
    #[must_use]
    pub fn is_graceful(&self) -> bool {
        !matches!(self, Self::Killed(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_kill_is_ungraceful() {
        assert!(StopOutcome::Drained(Duration::from_millis(12)).is_graceful());
        assert!(!StopOutcome::Killed(Duration::from_secs(10)).is_graceful());
    }
}
