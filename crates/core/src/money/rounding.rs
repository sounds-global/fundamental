/// Which way a figure goes when it does not land on a whole unit.
///
/// There is deliberately no half-to-even: a statement and its invoice must round alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rounding {
    /// Half a unit or more goes away from zero; less stays. The way an invoice rounds.
    HalfAwayFromZero,
    /// Any part of a unit at all goes away from zero: the honest direction for a cost.
    AwayFromZero,
}

impl Rounding {
    /// Whether a remainder of `remainder` out of `unit` micros moves the figure one unit
    /// further from zero.
    #[must_use]
    pub(crate) const fn carries(self, remainder: i64, unit: i64) -> bool {
        match self {
            // Doubling rather than halving: `remainder >= unit / 2` would round halves down for an odd unit.
            Self::HalfAwayFromZero => remainder * 2 >= unit,
            Self::AwayFromZero => remainder > 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_or_more_carries_and_less_does_not() {
        let mode = Rounding::HalfAwayFromZero;
        assert!(!mode.carries(4_999, 10_000));
        assert!(mode.carries(5_000, 10_000));
        assert!(mode.carries(9_999, 10_000));
        assert!(!mode.carries(0, 10_000));
    }

    #[test]
    fn any_part_of_a_unit_carries_when_the_direction_is_away_from_zero() {
        let mode = Rounding::AwayFromZero;
        assert!(!mode.carries(0, 10_000));
        assert!(mode.carries(1, 10_000));
        assert!(mode.carries(9_999, 10_000));
    }

    #[test]
    fn an_odd_unit_still_rounds_its_half_away_from_zero() {
        let mode = Rounding::HalfAwayFromZero;
        assert!(
            mode.carries(2, 3),
            "2 of 3 is more than half and must carry"
        );
        assert!(!mode.carries(1, 3), "1 of 3 is less than half and must not");
        assert!(mode.carries(1, 2));
    }
}
