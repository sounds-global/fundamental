use fundamental_core::AnyError;
use fundamental_macros::Error;

/// Errors a projection recompute handler returns; the refresh loop logs and skips them.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProjectionError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// Reading a source model to recompute the projection failed.
    #[error(
        code = "PROJ_001",
        message = "Projection source read failed: {0}",
        status = 500,
        retryable
    )]
    Read(String),
}
