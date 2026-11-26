//! [`SlotGuard`]: gives a connection's slot back however its session ends.

use std::sync::Arc;

use tracing::{debug, error};

use crate::session::CloseReason;
use crate::slots::SlotCell;

/// Owned by the session future, so the slot is released when the session ends, not when the last context clone drops.
#[derive(Debug)]
pub(crate) struct SlotGuard {
    cell: Arc<SlotCell>,
    reason: Option<CloseReason>,
}

impl SlotGuard {
    pub(crate) fn new(cell: Arc<SlotCell>) -> Self {
        Self { cell, reason: None }
    }

    pub(crate) fn cell(&self) -> &Arc<SlotCell> {
        &self.cell
    }

    /// Records why the connection ended; the slot is given back when the guard drops.
    pub(crate) fn ended(&mut self, reason: CloseReason) {
        self.reason = Some(reason);
    }
}

impl Drop for SlotGuard {
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn drop(&mut self) {
        let reason = self.reason.unwrap_or_else(|| {
            if self.cell.control().drain().is_expired() {
                debug!("session aborted at the end of the drain");
                CloseReason::DrainTimeout
            } else {
                error!(
                    "a connection's session ended without a close reason; it panicked or was aborted"
                );
                CloseReason::TransportError
            }
        });
        self.cell.close(reason);
    }
}
