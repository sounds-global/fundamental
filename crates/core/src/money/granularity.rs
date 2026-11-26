use crate::money::Denomination;

/// How fine a figure derived from an amount is allowed to be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Granularity {
    /// A millionth of a major unit: every digit the type holds.
    #[default]
    Micro,
    /// Four fractional digits, a hundredth of a cent: what a published per-unit rate is quoted at.
    TenThousandth,
    /// The denomination's own minor unit: what an invoice and a total a buyer reads are written in.
    Minor,
}

impl Granularity {
    /// How many micros one unit of this granularity is, in `denomination`.
    #[must_use]
    pub const fn micros_in(self, denomination: Denomination) -> i64 {
        match self {
            Self::Micro => 1,
            Self::TenThousandth => 100,
            Self::Minor => 10_i64.pow(6_u32.saturating_sub(denomination.minor_digits())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unit_is_a_whole_number_of_micros() {
        assert_eq!(Granularity::Micro.micros_in(Denomination::Eur), 1);
        assert_eq!(Granularity::TenThousandth.micros_in(Denomination::Eur), 100);
        assert_eq!(Granularity::Minor.micros_in(Denomination::Eur), 10_000);
    }

    #[test]
    fn the_minor_unit_is_derived_from_the_denomination() {
        for denomination in [Denomination::Eur, Denomination::Usd] {
            let expected = 10_i64.pow(6 - denomination.minor_digits());
            assert_eq!(Granularity::Minor.micros_in(denomination), expected);
        }
    }

    #[test]
    fn the_granularities_are_ordered_from_finest_to_coarsest() {
        let eur = Denomination::Eur;
        assert!(Granularity::Micro.micros_in(eur) < Granularity::TenThousandth.micros_in(eur));
        assert!(Granularity::TenThousandth.micros_in(eur) < Granularity::Minor.micros_in(eur));
    }
}
