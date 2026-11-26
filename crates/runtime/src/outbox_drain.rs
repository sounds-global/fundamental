//! [`OutboxDrain`]: what an [`OutboxSurface`](crate::OutboxSurface) runs on a tick.

use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;
use fundamental_core::outbox::{DrainReport, OutboxInventory};

/// Relays one batch of pending outbox rows, reporting what the tick did with them.
pub trait OutboxDrain: Send + Sync + 'static {
    /// Relays up to `batch` pending rows.
    ///
    /// # Errors
    /// [`AnyError`] if the batch could not be claimed; a per-row failure is recorded on the row.
    fn drain(
        &self,
        batch: u64,
    ) -> Pin<Box<dyn Future<Output = Result<DrainReport, AnyError>> + Send + '_>>;

    /// What the outbox holds that it will not send on its own: rows set aside, deferred or skipped.
    /// # Errors
    /// Returns [`AnyError`] if the store cannot be read.
    fn inventory(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<OutboxInventory, AnyError>> + Send + '_>>;
}
