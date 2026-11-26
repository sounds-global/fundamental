//! The [`IsRetryable`] trait used by the circuit breaker to classify failures.

/// Trait for determining if an error is retryable.
pub trait IsRetryable {
    /// Returns true if this error is retryable (transient).
    fn is_retryable(&self) -> bool;
}

impl IsRetryable for crate::CqlError {
    fn is_retryable(&self) -> bool {
        crate::CqlError::is_retryable(self)
    }
}

impl<E: IsRetryable> IsRetryable for Box<E> {
    fn is_retryable(&self) -> bool {
        (**self).is_retryable()
    }
}

impl<E: IsRetryable> IsRetryable for std::sync::Arc<E> {
    fn is_retryable(&self) -> bool {
        (**self).is_retryable()
    }
}
