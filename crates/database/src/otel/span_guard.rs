//! The [`DbSpanGuard`], which times a database operation and records its outcome on drop.

use std::time::Duration;

use tracing::{Span, error, warn};

use super::record_outcome;

/// A guard that records operation duration and outcome on drop.
#[derive(Debug)]
pub struct DbSpanGuard {
    span: Span,
    start: std::time::Instant,
    outcome_recorded: bool,
}

impl DbSpanGuard {
    /// Creates a new span guard.
    pub fn new(span: Span) -> Self {
        Self {
            span,
            start: std::time::Instant::now(),
            outcome_recorded: false,
        }
    }

    /// Returns a reference to the span.
    pub fn span(&self) -> &Span {
        &self.span
    }

    /// Returns the elapsed time since the span was created.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Marks the operation as successful.
    pub fn success(mut self) -> Duration {
        let duration = self.start.elapsed();
        record_outcome(&self.span, true, None);
        self.outcome_recorded = true;
        duration
    }

    /// Marks the operation as failed with an error message.
    pub fn error(mut self, message: &str) -> Duration {
        let duration = self.start.elapsed();
        record_outcome(&self.span, false, Some(message));
        // Emitted as an event too, so a log-only consumer sees the failure.
        error!(
            parent: &self.span,
            elapsed_ms = duration.as_millis(),
            message,
            "db operation failed"
        );
        self.outcome_recorded = true;
        duration
    }
}

impl Drop for DbSpanGuard {
    fn drop(&mut self) {
        // Dropped without an outcome (`?` or an unwind): record and log it, never silently.
        if !self.outcome_recorded {
            record_outcome(&self.span, false, Some("operation did not complete"));
            warn!(
                parent: &self.span,
                elapsed_ms = self.start.elapsed().as_millis(),
                "db span guard dropped without recorded outcome"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::info_span;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn drop_without_outcome_warns() {
        let guard = DbSpanGuard::new(info_span!("db.test"));
        drop(guard);
        assert!(logs_contain(
            "db span guard dropped without recorded outcome"
        ));
    }

    #[traced_test]
    #[test]
    fn error_emits_failure_event() {
        let guard = DbSpanGuard::new(info_span!("db.test"));
        let _ = guard.error("connection reset");
        assert!(logs_contain("db operation failed"));
        assert!(logs_contain("connection reset"));
        assert!(!logs_contain("dropped without recorded outcome"));
    }

    #[traced_test]
    #[test]
    fn success_is_silent() {
        let guard = DbSpanGuard::new(info_span!("db.test"));
        let _ = guard.success();
        assert!(!logs_contain("db operation failed"));
        assert!(!logs_contain("dropped without recorded outcome"));
    }
}
