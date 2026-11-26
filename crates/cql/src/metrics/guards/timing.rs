use fundamental_db::DatabaseMetrics;
use std::time::{Duration, Instant};

/// Timing guard for automatic metrics collection.
///
/// Records the duration when dropped.
pub struct TimingGuard<'a> {
    metrics: &'a dyn DatabaseMetrics,
    operation: &'a str,
    table: &'a str,
    slow_after: Duration,
    start: Instant,
    success: bool,
}

impl<'a> TimingGuard<'a> {
    /// Creates a new timing guard.
    pub fn new(
        metrics: &'a dyn DatabaseMetrics,
        operation: &'a str,
        table: &'a str,
        slow_after: Duration,
    ) -> Self {
        Self {
            metrics,
            operation,
            table,
            slow_after,
            start: Instant::now(),
            success: true,
        }
    }

    /// Marks the operation as failed.
    pub fn mark_failed(&mut self) {
        self.success = false;
    }

    /// Marks the operation as successful.
    pub fn mark_success(&mut self) {
        self.success = true;
    }
}

impl Drop for TimingGuard<'_> {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        self.metrics
            .record_query(self.operation, self.table, duration, self.success);

        if duration > self.slow_after {
            self.metrics
                .record_slow_query(self.operation, self.table, duration);
        }
    }
}

impl std::fmt::Debug for TimingGuard<'_> {
    /// The collector is not `Debug`; what is being measured is what a reader needs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimingGuard")
            .field("operation", &self.operation)
            .field("table", &self.table)
            .field("elapsed", &self.start.elapsed())
            .field("success", &self.success)
            .finish()
    }
}
