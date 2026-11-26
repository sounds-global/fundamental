//! The admin surface's typed failures ([`AdminError`]).

use fundamental_macros::Error;

/// What the admin surface answers when a probe endpoint must say no; `503` so a balancer retries elsewhere.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AdminError {
    /// One or more readiness probes could not reach their dependency.
    #[error(
        code = "ADMIN_001",
        message = "not ready: {failed}",
        status = 503,
        retryable
    )]
    NotReady {
        /// The comma-separated names of the probes that failed.
        failed: String,
    },

    /// The process is shutting down and must not be sent new work.
    #[error(
        code = "ADMIN_002",
        message = "draining: this instance is shutting down",
        status = 503,
        retryable
    )]
    Draining,

    /// A metrics scrape arrived but no renderer was registered.
    #[error(
        code = "ADMIN_003",
        message = "metrics are not configured on this surface",
        status = 404
    )]
    MetricsUnavailable,
}
