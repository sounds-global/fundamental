use std::sync::atomic::{AtomicU64, Ordering};

/// Counts the vault challenges a server holds and replaces, across all of its connections.
#[derive(Debug, Default)]
pub struct VaultChallengeGauge {
    pending: AtomicU64,
    replaced: AtomicU64,
}

impl VaultChallengeGauge {
    /// Creates a gauge with nothing held and nothing replaced.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The challenges held right now: at most one per open connection.
    #[must_use]
    pub fn pending(&self) -> u64 {
        self.pending.load(Ordering::Relaxed)
    }

    /// The challenges replaced by a newer one on the same connection before being answered,
    /// since the gauge was created.
    #[must_use]
    pub fn replaced(&self) -> u64 {
        self.replaced.load(Ordering::Relaxed)
    }

    pub(crate) fn hold(&self) {
        self.pending.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn release(&self) {
        self.pending.fetch_sub(1, Ordering::Relaxed);
    }

    pub(crate) fn count_replacement(&self) {
        self.replaced.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_gauge_holds_and_has_replaced_nothing() {
        let gauge = VaultChallengeGauge::new();
        assert_eq!(gauge.pending(), 0);
        assert_eq!(gauge.replaced(), 0);
    }

    #[test]
    fn hold_and_release_net_to_zero() {
        let gauge = VaultChallengeGauge::new();
        gauge.hold();
        gauge.hold();
        assert_eq!(gauge.pending(), 2);
        gauge.release();
        gauge.release();
        assert_eq!(gauge.pending(), 0);
    }

    #[test]
    fn replacements_accumulate() {
        let gauge = VaultChallengeGauge::new();
        gauge.count_replacement();
        gauge.count_replacement();
        assert_eq!(gauge.replaced(), 2);
    }
}
