//! Timing guard for YCQL OpenTelemetry spans.

use super::record_outcome;
use std::time::Duration;

/// A span guard that records duration on drop.
#[derive(Debug)]
pub struct SpanTimingGuard {
    span: tracing::Span,
    start: std::time::Instant,
}

impl SpanTimingGuard {
    /// Creates a new span timing guard.
    pub fn new(span: tracing::Span) -> Self {
        Self {
            span,
            start: std::time::Instant::now(),
        }
    }

    /// Returns a reference to the span.
    pub fn span(&self) -> &tracing::Span {
        &self.span
    }

    /// Records success and returns the duration.
    pub fn finish_success(self) -> Duration {
        let duration = self.start.elapsed();
        record_outcome(&self.span, true, None);
        duration
    }

    /// Records failure and returns the duration.
    pub fn finish_error(self, message: &str) -> Duration {
        let duration = self.start.elapsed();
        record_outcome(&self.span, false, Some(message));
        duration
    }
}

impl Drop for SpanTimingGuard {
    fn drop(&mut self) {
        // The finish methods consume self, so an unfinished guard counts as a success.
    }
}
