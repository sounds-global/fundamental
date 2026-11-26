use fundamental_core::AnyError;
use fundamental_macros::Error;

/// Errors from circuit breaker operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CircuitBreakerError<E> {
    /// Catch-all for errors that do not fit a more specific variant.
    Other(AnyError),
    /// Circuit is open, request rejected without execution.
    #[error(
        code = "SQLCIRCUIT_001",
        message = "Circuit breaker is open",
        status = 503
    )]
    Open,

    /// Underlying operation failed.
    #[error(
        code = "SQLCIRCUIT_002",
        message = "Operation failed: {0}",
        status = 500
    )]
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
            _ => None,
        }
    }
}
