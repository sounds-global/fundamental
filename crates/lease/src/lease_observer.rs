use std::time::Duration;

use crate::{LeaseName, LossReason};

/// Hooks for a lease's handovers, called by a [`LeasedSurface`](crate::LeasedSurface); both default to nothing.
pub trait LeaseObserver: Send + Sync + 'static {
    /// A tenure began at `epoch`, after `handover_gap` unheld (`None` for a row's first tenure).
    fn acquired(&self, lease: LeaseName, epoch: i64, handover_gap: Option<Duration>) {
        let _ = (lease, epoch, handover_gap);
    }

    /// The tenure at `epoch` ended, for `reason`.
    fn lost(&self, lease: LeaseName, epoch: i64, reason: LossReason) {
        let _ = (lease, epoch, reason);
    }
}
