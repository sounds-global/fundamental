//! Error type for the change-capture and fan-out path.

use fundamental_core::AnyError;
use fundamental_macros::Error;

/// The result of a change-capture or fan-out operation.
pub type ChangeResult<T> = Result<T, ChangeError>;

/// Errors produced while publishing or receiving captured [`SyncChange`]s.
///
/// [`SyncChange`]: crate::change::SyncChange
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ChangeError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// The underlying transport rejected a published change.
    #[error(
        code = "SYNC_010",
        message = "Change publish failed: {0}",
        status = 500,
        retryable
    )]
    Publish(String),

    /// This subscriber fell behind and changes were dropped; the next `recv` resumes, a re-pull closes
    /// the gap.
    #[error(
        code = "SYNC_011",
        message = "Change subscriber lagged; {0} change(s) dropped",
        status = 500
    )]
    Lagged(u64),

    /// The publisher was dropped, so no further changes will ever arrive.
    #[error(code = "SYNC_012", message = "Change stream closed", status = 500)]
    Closed,
}
