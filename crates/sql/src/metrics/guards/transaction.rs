use fundamental_db::DatabaseMetrics;
use std::time::Instant;

/// Transaction timing guard for automatic transaction metrics.
pub struct TransactionTimingGuard<'a> {
    metrics: &'a dyn DatabaseMetrics,
    start: Instant,
    committed: Option<bool>,
}

impl<'a> TransactionTimingGuard<'a> {
    /// Creates a new transaction timing guard.
    pub fn new(metrics: &'a dyn DatabaseMetrics) -> Self {
        metrics.record_transaction_start();
        Self {
            metrics,
            start: Instant::now(),
            committed: None,
        }
    }

    /// Marks the transaction as committed.
    pub fn mark_committed(&mut self) {
        self.committed = Some(true);
    }

    /// Marks the transaction as rolled back.
    pub fn mark_rolled_back(&mut self) {
        self.committed = Some(false);
    }
}

impl Drop for TransactionTimingGuard<'_> {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        // Dropped without an outcome is a rollback, or the commit rate would read better than it is.
        self.metrics
            .record_transaction(self.committed.unwrap_or(false), duration);
    }
}

impl std::fmt::Debug for TransactionTimingGuard<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransactionTimingGuard")
            .field("elapsed", &self.start.elapsed())
            .field("committed", &self.committed)
            .finish()
    }
}
