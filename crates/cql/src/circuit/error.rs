//! The [`CircuitBreakerError`] type returned by circuit breaker calls.

use fundamental_macros::Error;

/// Errors from circuit breaker operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CircuitBreakerError<E> {
    /// Circuit is open, request rejected without execution.
    #[error("Circuit breaker is open")]
    Open,

    /// Underlying operation failed.
    #[error("Operation failed: {0}")]
    Inner(#[source] E),
}

impl<E> CircuitBreakerError<E> {
    /// Returns true if the circuit is open.
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Open)
    }

    /// Returns the inner error if present.
    pub fn inner(&self) -> Option<&E> {
        match self {
            Self::Inner(e) => Some(e),
            Self::Open => None,
        }
    }
}
