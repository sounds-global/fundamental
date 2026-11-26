//! [`OutboxObserver`]: what a service learns from each drain tick.

use fundamental_core::outbox::{DrainReport, OutboxInventory};

/// Reports the outcome of every [`OutboxSurface`](crate::OutboxSurface) tick.
///
/// Implementations must be cheap and non-blocking: they run on the drain loop's own task.
pub trait OutboxObserver: Send + Sync + 'static {
    /// A completed tick: what was claimed and relayed out of the `batch` asked for, and what was held.
    fn drained(&self, surface: &'static str, report: &DrainReport, batch: u64);

    /// A tick that could not claim its batch at all.
    fn drain_failed(&self, surface: &'static str);

    /// What the outbox holds that its relay will not send, reported at start and on every inventory interval.
    fn inventoried(&self, surface: &'static str, inventory: &OutboxInventory);

    /// An inventory that could not be read.
    fn inventory_failed(&self, surface: &'static str);
}
