//! [`DayStyle`] — a day of the year with its weekday and without its year.

/// A day with its weekday and without its year.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DayStyle {
    /// Weekday and month spelled out: `donderdag 8 oktober`, `Thursday 8 October`.
    Long,
    /// Weekday and month abbreviated: `do 8 okt`, `Thu 8 Oct`.
    #[default]
    Medium,
}
