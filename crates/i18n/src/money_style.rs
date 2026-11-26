//! [`MoneyStyle`] — how many fractional digits a rendered amount shows.

/// How many fractional digits a rendered amount shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MoneyStyle {
    /// Every digit the amount holds, at least the minor digits: `€ 99,00`, `€ 0,0025`.
    #[default]
    Exact,
    /// No fractional digits for a whole amount, otherwise as [`Exact`](Self::Exact): `€ 2.500`.
    WholeWhenIntegral,
}
