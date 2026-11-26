//! [`DateStyle`] — how much of a calendar date is written, in CLDR's four date formats.

/// How much of a calendar date is written: CLDR's four date formats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DateStyle {
    /// Weekday, day, month and year, spelled out: `donderdag 8 oktober 2026`,
    /// `Thursday, 8 October 2026`.
    Full,
    /// Day, month and year, the month spelled out: `8 oktober 2026`, `8 October 2026`.
    Long,
    /// Day, month and year, the month abbreviated: `8 okt 2026`, `8 Oct 2026`.
    #[default]
    Medium,
    /// Numeric: `08-10-2026`, `08/10/2026`.
    Short,
}
