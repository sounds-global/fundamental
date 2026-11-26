use core::num::NonZeroU64;

use crate::money::{Granularity, Money, Rounding};

/// An amount divided into equal parts: what each part gets, and what does not divide.
///
/// `each × parts + remainder` is exactly the amount divided; the remainder has the dividend's sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct Division {
    each: Money,
    remainder: Money,
    parts: NonZeroU64,
    unit: Granularity,
}

impl Division {
    /// A division of `amount` into `parts` whole `unit`s.
    ///
    /// Private: the invariants are established only by [`Money::checked_div`].
    pub(crate) const fn of(
        each: Money,
        remainder: Money,
        parts: NonZeroU64,
        unit: Granularity,
    ) -> Self {
        Self {
            each,
            remainder,
            parts,
            unit,
        }
    }

    /// What each part gets: a whole number of [`Granularity`] units, truncated toward
    /// zero; exact only when [`Division::remainder`] is zero.
    #[must_use]
    pub const fn each(self) -> Money {
        self.each
    }

    /// What did not divide, with the dividend's sign.
    #[must_use]
    pub const fn remainder(self) -> Money {
        self.remainder
    }

    /// How many parts the amount was divided into.
    #[must_use]
    pub const fn parts(self) -> NonZeroU64 {
        self.parts
    }

    /// The granularity [`Division::each`] is a whole number of.
    #[must_use]
    pub const fn unit(self) -> Granularity {
        self.unit
    }

    /// Whether nothing was left over.
    #[must_use]
    pub const fn is_exact(self) -> bool {
        self.remainder.micros() == 0
    }

    /// What each part gets when the amount divides evenly, and `None` when it does not.
    #[must_use]
    pub const fn exact(self) -> Option<Money> {
        if self.is_exact() {
            Some(self.each)
        } else {
            None
        }
    }

    /// What each part gets with the remainder folded back in, `rounding` deciding which
    /// way, decided against the exact remainder.
    #[must_use]
    pub const fn rounded(self, rounding: Rounding) -> Money {
        let unit = self.unit.micros_in(self.each.denomination());
        // Compared as a fraction rather than divided, because dividing would round a second time.
        let magnitude = self.remainder.micros().unsigned_abs();
        let parts = self.parts.get();
        // `magnitude < parts * unit` by construction, so this product stays inside u128.
        let carries = match rounding {
            Rounding::HalfAwayFromZero => {
                (magnitude as u128) * 2 >= (parts as u128) * (unit as u128)
            }
            Rounding::AwayFromZero => magnitude > 0,
        };
        if !carries {
            return self.each;
        }
        let step = if self.remainder.micros() < 0 {
            -unit
        } else {
            unit
        };
        Money::from_micros(
            self.each.micros().saturating_add(step),
            self.each.denomination(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Denomination;

    fn eur(micros: i64) -> Money {
        Money::from_micros(micros, Denomination::Eur)
    }

    fn parts(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).expect("a non-zero count")
    }

    #[test]
    fn the_parts_and_the_remainder_add_back_to_the_amount() {
        for (amount, count) in [
            (99_000_000_i64, 3_500_u64),
            (499_000_000, 20_000),
            (1, 7),
            (-99_000_000, 3_500),
            (0, 3),
        ] {
            let division = eur(amount)
                .checked_div(parts(count), Granularity::TenThousandth)
                .expect("a divisible amount");
            let rebuilt = division.each().micros() * i64::try_from(count).expect("a small count")
                + division.remainder().micros();
            assert_eq!(rebuilt, amount, "{amount} over {count} did not reconcile");
        }
    }

    #[test]
    fn a_per_unit_price_rounds_the_way_an_invoice_rounds() {
        let starter = eur(99_000_000)
            .checked_div(parts(3_500), Granularity::TenThousandth)
            .expect("a divisible amount");
        assert_eq!(starter.each().micros(), 28_200, "truncated toward zero");
        assert_eq!(
            starter.rounded(Rounding::HalfAwayFromZero).micros(),
            28_300,
            "28 285,71 micros is nearer 28 300 than 28 200 at this granularity"
        );

        let pro = eur(499_000_000)
            .checked_div(parts(20_000), Granularity::TenThousandth)
            .expect("a divisible amount");
        assert_eq!(pro.each().micros(), 24_900);
        assert_eq!(pro.rounded(Rounding::HalfAwayFromZero).micros(), 25_000);
    }

    #[test]
    fn any_part_of_a_unit_carries_when_the_caller_asks_for_away_from_zero() {
        let division = eur(100_001)
            .checked_div(parts(1), Granularity::Minor)
            .expect("a divisible amount");
        assert_eq!(
            division.rounded(Rounding::HalfAwayFromZero).micros(),
            100_000
        );
        assert_eq!(division.rounded(Rounding::AwayFromZero).micros(), 110_000);
    }

    #[test]
    fn a_negative_amount_keeps_its_sign_through_the_division() {
        let division = eur(-99_000_000)
            .checked_div(parts(3_500), Granularity::TenThousandth)
            .expect("a divisible amount");
        assert!(division.each().micros() < 0);
        assert!(division.remainder().micros() < 0);
        assert_eq!(
            division.rounded(Rounding::HalfAwayFromZero).micros(),
            -28_300
        );
    }

    #[test]
    fn an_exact_division_reports_itself_exact() {
        let division = eur(100_000)
            .checked_div(parts(10), Granularity::Micro)
            .expect("a divisible amount");
        assert!(division.is_exact());
        assert_eq!(division.exact(), Some(eur(10_000)));
        assert_eq!(division.rounded(Rounding::HalfAwayFromZero), eur(10_000));

        let inexact = eur(1)
            .checked_div(parts(3), Granularity::Micro)
            .expect("a divisible amount");
        assert!(!inexact.is_exact());
        assert_eq!(inexact.exact(), None);
    }

    #[test]
    fn rounding_is_decided_once_against_the_exact_remainder() {
        let division = eur(14_000)
            .checked_div(parts(1), Granularity::Minor)
            .expect("a divisible amount");
        assert_eq!(division.each().micros(), 10_000);
        assert_eq!(division.remainder().micros(), 4_000);
        assert_eq!(
            division.rounded(Rounding::HalfAwayFromZero).micros(),
            10_000
        );
    }
}
