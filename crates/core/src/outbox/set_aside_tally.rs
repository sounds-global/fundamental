//! [`SetAsideTally`] — how many rows one claim set aside, by why.

use super::RefusalKind;

/// How many rows one claim set aside, counted by [`RefusalKind`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SetAsideTally {
    counts: [u64; RefusalKind::ALL.len()],
}

impl SetAsideTally {
    /// Counts one more row set aside for `kind`.
    pub fn record(&mut self, kind: RefusalKind) {
        let slot = &mut self.counts[kind.index()];
        *slot = slot.saturating_add(1);
    }

    /// How many rows were set aside for `kind`.
    #[must_use]
    pub fn count(&self, kind: RefusalKind) -> u64 {
        self.counts[kind.index()]
    }

    /// How many rows were set aside for any reason.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.counts
            .iter()
            .fold(0, |sum, count| sum.saturating_add(*count))
    }

    /// Both tallies together, kind by kind.
    #[must_use]
    pub fn plus(&self, other: &Self) -> Self {
        let mut counts = self.counts;
        for (count, more) in counts.iter_mut().zip(other.counts) {
            *count = count.saturating_add(more);
        }
        Self { counts }
    }

    /// Each kind that set a row aside, with its count.
    pub fn nonzero(&self) -> impl Iterator<Item = (RefusalKind, u64)> + '_ {
        RefusalKind::ALL
            .into_iter()
            .map(|kind| (kind, self.count(kind)))
            .filter(|(_, count)| *count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::{RefusalKind, SetAsideTally};

    #[test]
    fn two_tallies_add_kind_by_kind() {
        let mut left = SetAsideTally::default();
        left.record(RefusalKind::DecryptRefused);
        let mut right = SetAsideTally::default();
        right.record(RefusalKind::DecryptRefused);
        right.record(RefusalKind::ValueRefused);
        let both = left.plus(&right);
        assert_eq!(both.count(RefusalKind::DecryptRefused), 2);
        assert_eq!(both.count(RefusalKind::ValueRefused), 1);
        assert_eq!(both.total(), 3);
    }

    #[test]
    fn an_empty_tally_holds_nothing() {
        let tally = SetAsideTally::default();
        assert_eq!(tally.total(), 0);
        assert_eq!(tally.nonzero().count(), 0);
    }

    #[test]
    fn each_kind_is_counted_apart_and_in_the_total() {
        let mut tally = SetAsideTally::default();
        tally.record(RefusalKind::DecryptRefused);
        tally.record(RefusalKind::DecryptRefused);
        tally.record(RefusalKind::ValueRefused);
        assert_eq!(tally.count(RefusalKind::DecryptRefused), 2);
        assert_eq!(tally.count(RefusalKind::ValueRefused), 1);
        assert_eq!(tally.count(RefusalKind::KeyNotHeld), 0);
        assert_eq!(tally.total(), 3);
        assert_eq!(
            tally.nonzero().collect::<Vec<_>>(),
            [
                (RefusalKind::DecryptRefused, 2),
                (RefusalKind::ValueRefused, 1)
            ]
        );
    }
}
