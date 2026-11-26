//! [`Relay`]: the caller-supplied side effect the outbox drain runs per row.

use std::future::Future;
use std::time::Duration;

use fundamental_core::AnyError;

/// Relays one pending outbox row to its destination and durably marks it relayed.
///
/// It performs the side effect and commits the row's relayed transition itself. The effect
/// must be idempotent under retry, since a crash between effect and mark relays the row again.
pub trait Relay<R>: Send + Sync {
    /// Relays `row` and marks it relayed, or returns why it could not.
    ///
    /// # Errors
    /// Returns [`AnyError`] if the side effect or the mark-relayed commit fails; the
    /// drain then records the failure on the row and leaves it pending.
    fn relay(&self, row: &R) -> impl Future<Output = Result<(), AnyError>> + Send;

    /// The period this relay re-schedules a row it relayed by, when it keeps the row queued
    /// instead of marking it relayed (a recurring job); `None`, the default, otherwise.
    fn recurrence(&self) -> Option<Duration> {
        None
    }
}
