//! [`HeldCount`] — a count that says whether it stopped at its cap.

/// A count of rows, stating whether it is exact or stopped at the cap its query was given.
///
/// A capped answer must never read as the true number, nor as zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldCount {
    /// The count is exact.
    Exact(u64),
    /// At least this many rows are held; the query stopped at its cap.
    AtLeast(u64),
}

impl HeldCount {
    /// The count a query returned, given the cap it ran under.
    #[must_use]
    pub fn under_cap(counted: u64, cap: u64) -> Self {
        if counted >= cap {
            Self::AtLeast(cap)
        } else {
            Self::Exact(counted)
        }
    }

    /// The number of rows certainly held.
    #[must_use]
    pub fn at_least(self) -> u64 {
        match self {
            Self::Exact(count) | Self::AtLeast(count) => count,
        }
    }

    /// Whether no row is held.
    #[must_use]
    pub fn is_none_held(self) -> bool {
        matches!(self, Self::Exact(0))
    }

    /// Both counts together: a lower bound if either one is.
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        let total = self.at_least().saturating_add(other.at_least());
        match (self, other) {
            (Self::Exact(_), Self::Exact(_)) => Self::Exact(total),
            _ => Self::AtLeast(total),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HeldCount;

    #[test]
    fn a_count_below_its_cap_is_exact() {
        assert_eq!(HeldCount::under_cap(7, 10_000), HeldCount::Exact(7));
    }

    #[test]
    fn a_count_that_reaches_its_cap_is_a_lower_bound() {
        assert_eq!(
            HeldCount::under_cap(10_000, 10_000),
            HeldCount::AtLeast(10_000)
        );
        assert_eq!(HeldCount::under_cap(10_000, 10_000).at_least(), 10_000);
    }

    #[test]
    fn two_exact_counts_add_to_an_exact_count() {
        assert_eq!(
            HeldCount::Exact(2).plus(HeldCount::Exact(3)),
            HeldCount::Exact(5)
        );
    }

    #[test]
    fn a_sum_with_a_capped_count_is_a_lower_bound() {
        assert_eq!(
            HeldCount::Exact(2).plus(HeldCount::AtLeast(100)),
            HeldCount::AtLeast(102)
        );
        assert_eq!(
            HeldCount::AtLeast(100).plus(HeldCount::Exact(0)),
            HeldCount::AtLeast(100)
        );
    }

    #[test]
    fn only_an_exact_zero_holds_nothing() {
        assert!(HeldCount::Exact(0).is_none_held());
        assert!(!HeldCount::Exact(1).is_none_held());
        assert!(!HeldCount::AtLeast(0).is_none_held());
    }
}
