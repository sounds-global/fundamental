//! [`ProcessRefusalBudget`]: how many refusal warnings the whole server may write per window.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use tokio::time::Instant;
use tracing::warn;

use crate::observability::{FixedWindow, millis};

pub(crate) const PROCESS_WINDOW: Duration = Duration::from_secs(10);
/// Refusal warnings the whole server writes per [`PROCESS_WINDOW`].
pub(crate) const PROCESS_LINES_PER_WINDOW: u64 = 100;

/// The server-wide cap on refusal warnings; what it withholds is reported by [`flush`](Self::flush).
#[derive(Debug)]
pub(crate) struct ProcessRefusalBudget {
    window: Mutex<FixedWindow>,
    suppressed_since_flush: AtomicU64,
}

impl ProcessRefusalBudget {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            window: Mutex::new(FixedWindow::new(
                now,
                PROCESS_WINDOW,
                PROCESS_LINES_PER_WINDOW,
            )),
            suppressed_since_flush: AtomicU64::new(0),
        }
    }

    /// Whether one more warning fits the window open at `now`.
    pub(crate) fn permit(&self, now: Instant) -> bool {
        if self.lock().permit(now) {
            true
        } else {
            self.suppressed_since_flush.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    /// Logs how many warnings the budget withheld since the last flush, and returns that count.
    pub(crate) fn flush(&self) -> u64 {
        let suppressed = self.suppressed_since_flush.swap(0, Ordering::Relaxed);
        if suppressed > 0 {
            warn!(
                suppressed,
                window_ms = millis(PROCESS_WINDOW),
                limit = PROCESS_LINES_PER_WINDOW,
                "refusal warnings suppressed by the server-wide budget"
            );
        }
        suppressed
    }

    fn lock(&self) -> MutexGuard<'_, FixedWindow> {
        self.window.lock().unwrap_or_else(|poisoned| {
            warn!("server refusal budget lock was poisoned by a panic; recovering its window");
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_admits_its_allowance_and_counts_the_rest() {
        let origin = Instant::now();
        let budget = ProcessRefusalBudget::new(origin);
        let admitted = (0..PROCESS_LINES_PER_WINDOW + 25)
            .filter(|_| budget.permit(origin))
            .count();
        assert_eq!(admitted as u64, PROCESS_LINES_PER_WINDOW);
        assert_eq!(budget.flush(), 25);
        assert_eq!(budget.flush(), 0, "a flush reports each suppression once");
    }

    #[test]
    fn the_next_window_starts_a_new_allowance() {
        let origin = Instant::now();
        let budget = ProcessRefusalBudget::new(origin);
        for _ in 0..PROCESS_LINES_PER_WINDOW {
            assert!(budget.permit(origin));
        }
        assert!(!budget.permit(origin));
        assert!(budget.permit(origin + PROCESS_WINDOW));
    }
}
