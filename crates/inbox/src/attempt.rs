//! [`Attempt`] — how one run of an effect's transaction ended, told apart by type.

use fundamental_db::DatabaseError;
use fundamental_macros::Error;

/// The error an effect's transaction body ends with.
#[derive(Debug, Error)]
pub(crate) enum Attempt<E>
where
    E: fundamental_core::Error + std::error::Error + 'static,
{
    /// The inbox row was already there: the event was applied before.
    #[error(
        code = "SINBOX_017",
        message = "The event was already applied",
        status = 409
    )]
    AlreadyRecorded,

    /// One of the inbox's own statements failed, or the commit did.
    #[error(transparent)]
    Store(#[from] DatabaseError),

    /// The handler's effect failed.
    #[error(transparent)]
    Effect(E),
}
