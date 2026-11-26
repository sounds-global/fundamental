//! [`RuntimeError`], the error returned by the surface supervisor.

use fundamental_core::AnyError;
use fundamental_macros::Error;

/// Result alias for fallible runtime-supervisor operations.
pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// An error from supervising application surfaces.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RuntimeError {
    /// A lower-layer Fundamental error from a surface, preserved losslessly.
    Other(AnyError),

    /// A surface failed to start; surfaces started before it were drained first.
    #[error(
        code = "RUN_001",
        message = "Surface '{surface}' failed to start",
        status = 500
    )]
    Start {
        /// The surface that failed to start.
        surface: &'static str,
        /// The surface's own error.
        #[source]
        source: AnyError,
    },

    /// A surface ended mid-run with an error, bringing the process down.
    #[error(code = "RUN_002", message = "Surface '{surface}' failed", status = 500)]
    SurfaceFailed {
        /// The surface that failed.
        surface: &'static str,
        /// The surface's own error.
        #[source]
        source: AnyError,
    },

    /// The shutdown-signal handler could not be registered or awaited.
    #[error(
        code = "RUN_003",
        message = "Failed to listen for the shutdown signal",
        status = 500
    )]
    Signal {
        /// The underlying IO error from the signal handler.
        #[source]
        source: std::io::Error,
    },

    /// The bounded shutdown drain did not finish in time; surfaces were force-stopped.
    #[error(
        code = "RUN_004",
        message = "Shutdown drain exceeded {timeout_ms}ms; surfaces were force-stopped",
        status = 500
    )]
    DrainTimeout {
        /// The elapsed drain budget, in milliseconds.
        timeout_ms: u64,
    },

    /// The drains through this surface need longer than the shutdown budget; refused before any surface starts.
    #[error(
        code = "RUN_005",
        message = "The drain through surface '{surface}' needs {needed_ms}ms, which the {budget_ms}ms shutdown budget cannot fit",
        status = 500
    )]
    DrainBudget {
        /// The surface whose drain first reaches the budget.
        surface: &'static str,
        /// The longest the drains through this surface can take, in milliseconds.
        needed_ms: u64,
        /// The runtime's shutdown budget, in milliseconds.
        budget_ms: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    fn start_carries_stable_code_and_status() {
        let err = RuntimeError::Start {
            surface: "worker",
            source: AnyError::new(RuntimeError::DrainTimeout { timeout_ms: 1 }),
        };
        assert_eq!(err.error_code(), "RUN_001");
        assert_eq!(err.http_status_code(), 500);
        assert!(!err.is_retryable());
        assert!(err.to_string().contains("worker"));
    }

    #[test]
    fn surface_failed_names_the_surface() {
        let err = RuntimeError::SurfaceFailed {
            surface: "socket",
            source: AnyError::new(RuntimeError::DrainTimeout { timeout_ms: 1 }),
        };
        assert_eq!(err.error_code(), "RUN_002");
        assert!(err.to_string().contains("socket"));
    }

    #[test]
    fn signal_reports_the_io_failure() {
        let err = RuntimeError::Signal {
            source: std::io::Error::other("no handler"),
        };
        assert_eq!(err.error_code(), "RUN_003");
        assert_eq!(err.http_status_code(), 500);
    }

    #[test]
    fn drain_timeout_reports_the_budget() {
        let err = RuntimeError::DrainTimeout { timeout_ms: 250 };
        assert_eq!(err.error_code(), "RUN_004");
        assert!(err.to_string().contains("250"));
    }

    #[test]
    fn drain_budget_names_the_surface_and_both_durations() {
        let err = RuntimeError::DrainBudget {
            surface: "socket",
            needed_ms: 31_000,
            budget_ms: 30_000,
        };
        assert_eq!(err.error_code(), "RUN_005");
        assert_eq!(err.http_status_code(), 500);
        assert!(!err.is_retryable());
        let message = err.to_string();
        assert!(
            message.contains("socket") && message.contains("31000") && message.contains("30000")
        );
    }

    #[test]
    fn other_catch_all_delegates_to_the_wrapped_error() {
        let err: RuntimeError = AnyError::new(RuntimeError::DrainTimeout { timeout_ms: 5 }).into();
        assert_eq!(err.error_code(), "RUN_004");
        assert_eq!(err.http_status_code(), 500);
    }
}
