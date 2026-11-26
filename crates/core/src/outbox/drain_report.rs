//! [`DrainReport`] — what one outbox drain tick did with the rows it was handed.

use super::SetAsideTally;

/// What one outbox drain tick did with the rows it took.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DrainReport {
    /// Rows the claim leased to this tick.
    pub claimed: usize,
    /// Rows relayed and marked done.
    pub relayed: usize,
    /// Rows that refused to decode for a reason of their own, set aside in the claim.
    pub set_aside: SetAsideTally,
    /// Rows sealed under a key this process does not hold, left for a process that does.
    pub skipped: u64,
}

impl DrainReport {
    /// Both reports together, field by field.
    #[must_use]
    pub fn plus(&self, other: &Self) -> Self {
        Self {
            claimed: self.claimed.saturating_add(other.claimed),
            relayed: self.relayed.saturating_add(other.relayed),
            set_aside: self.set_aside.plus(&other.set_aside),
            skipped: self.skipped.saturating_add(other.skipped),
        }
    }

    /// Whether the tick took as many rows as it asked for, so the queue may not be empty.
    #[must_use]
    pub fn saturated(&self, batch: u64) -> bool {
        let claimed = u64::try_from(self.claimed).unwrap_or(u64::MAX);
        claimed
            .saturating_add(self.set_aside.total())
            .saturating_add(self.skipped)
            >= batch
    }
}

#[cfg(test)]
mod tests {
    use super::DrainReport;
    use crate::outbox::{RefusalKind, SetAsideTally};

    #[test]
    fn two_ticks_add_field_by_field() {
        let mut set_aside = SetAsideTally::default();
        set_aside.record(RefusalKind::ValueRefused);
        let first = DrainReport {
            claimed: 3,
            relayed: 2,
            set_aside,
            skipped: 1,
        };
        let second = DrainReport {
            claimed: 4,
            relayed: 4,
            set_aside,
            skipped: 0,
        };
        let both = first.plus(&second);
        assert_eq!(both.claimed, 7);
        assert_eq!(both.relayed, 6);
        assert_eq!(both.set_aside.count(RefusalKind::ValueRefused), 2);
        assert_eq!(both.skipped, 1);
    }

    #[test]
    fn a_tick_short_of_its_batch_is_not_saturated() {
        let report = DrainReport {
            claimed: 3,
            relayed: 3,
            ..DrainReport::default()
        };
        assert!(!report.saturated(10));
    }

    #[test]
    fn a_tick_filled_by_rows_set_aside_counts_as_saturated() {
        let mut set_aside = SetAsideTally::default();
        for _ in 0..8 {
            set_aside.record(RefusalKind::DecryptRefused);
        }
        let report = DrainReport {
            claimed: 2,
            relayed: 2,
            set_aside,
            skipped: 0,
        };
        assert!(report.saturated(10));
    }

    #[test]
    fn a_tick_filled_by_skipped_rows_counts_as_saturated() {
        let report = DrainReport {
            skipped: 10,
            ..DrainReport::default()
        };
        assert!(report.saturated(10));
    }
}
