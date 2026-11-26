use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Counts one caller as waiting for a connection, for exactly as long as it waits.
///
/// A guard, so a cancelled acquire still decrements; `Relaxed`, as the count only feeds a gauge.
pub(crate) struct AcquireGuard {
    waiters: Arc<AtomicU32>,
}

impl AcquireGuard {
    pub(crate) fn held(waiters: &Arc<AtomicU32>) -> Self {
        waiters.fetch_add(1, Ordering::Relaxed);
        Self {
            waiters: Arc::clone(waiters),
        }
    }
}

impl Drop for AcquireGuard {
    fn drop(&mut self) {
        // Holds at zero rather than wrapping a `u32` to four billion on a defect elsewhere.
        let _ = self
            .waiters
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |waiting| {
                Some(waiting.saturating_sub(1))
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guard_counts_for_as_long_as_it_is_held() {
        let waiters = Arc::new(AtomicU32::new(0));
        {
            let _first = AcquireGuard::held(&waiters);
            assert_eq!(waiters.load(Ordering::Relaxed), 1);
            let _second = AcquireGuard::held(&waiters);
            assert_eq!(waiters.load(Ordering::Relaxed), 2);
        }
        assert_eq!(waiters.load(Ordering::Relaxed), 0, "both guards released");
    }

    #[test]
    fn a_dropped_guard_releases_even_though_nothing_after_it_ran() {
        let waiters = Arc::new(AtomicU32::new(0));
        let guard = AcquireGuard::held(&waiters);
        assert_eq!(waiters.load(Ordering::Relaxed), 1);
        drop(guard);
        assert_eq!(waiters.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn releasing_below_zero_holds_at_zero_rather_than_wrapping() {
        let waiters = Arc::new(AtomicU32::new(0));
        let guard = AcquireGuard::held(&waiters);
        waiters.store(0, Ordering::Relaxed);
        drop(guard);
        assert_eq!(
            waiters.load(Ordering::Relaxed),
            0,
            "a wrapped count would read as four billion waiters"
        );
    }
}
