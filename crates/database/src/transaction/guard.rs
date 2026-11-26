//! Transaction completion guard.

use tracing::warn;

/// A guard that ensures a transaction is rolled back if it is not committed.
///
/// `Drop` cannot await a rollback, so an uncommitted guard logs a warning and drops the inner
/// value, and the backend rolls back on connection drop. [`take`](Self::take) or
/// [`mark_committed`](Self::mark_committed) suppress the warning.
#[derive(Debug)]
pub struct TransactionGuard<T> {
    inner: Option<T>,
    committed: bool,
}

impl<T> TransactionGuard<T> {
    /// Creates a new transaction guard.
    pub fn new(transaction: T) -> Self {
        Self {
            inner: Some(transaction),
            committed: false,
        }
    }

    /// Returns a reference to the inner transaction.
    pub fn inner(&self) -> Option<&T> {
        self.inner.as_ref()
    }

    /// Returns a mutable reference to the inner transaction.
    pub fn inner_mut(&mut self) -> Option<&mut T> {
        self.inner.as_mut()
    }

    /// Takes the inner transaction, marking the guard as complete.
    pub fn take(mut self) -> Option<T> {
        self.committed = true;
        self.inner.take()
    }

    /// Marks the transaction as committed without taking it.
    pub fn mark_committed(&mut self) {
        self.committed = true;
    }
}

impl<T> Drop for TransactionGuard<T> {
    fn drop(&mut self) {
        // `Drop` cannot await a rollback: log the missed commit and let the backend roll back on drop.
        if !self.committed && self.inner.is_some() {
            warn!(
                committed = false,
                "transaction guard dropped without commit; rolling back"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    struct FakeTx;

    #[traced_test]
    #[test]
    fn drop_without_commit_warns_and_rolls_back() {
        let guard = TransactionGuard::new(FakeTx);
        drop(guard);
        assert!(logs_contain(
            "transaction guard dropped without commit; rolling back"
        ));
    }

    #[traced_test]
    #[test]
    fn take_suppresses_rollback_warning() {
        let guard = TransactionGuard::new(FakeTx);
        let inner = guard.take();
        assert!(inner.is_some());
        assert!(!logs_contain("dropped without commit"));
    }

    #[traced_test]
    #[test]
    fn mark_committed_suppresses_rollback_warning() {
        let mut guard = TransactionGuard::new(FakeTx);
        guard.mark_committed();
        drop(guard);
        assert!(!logs_contain("dropped without commit"));
    }

    #[test]
    fn inner_accessors_expose_the_transaction() {
        let mut guard = TransactionGuard::new(FakeTx);
        assert!(guard.inner().is_some());
        assert!(guard.inner_mut().is_some());
        guard.mark_committed();
    }
}
