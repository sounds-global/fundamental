//! [`TimeStyle`] — how much of a wall-clock time is written, in CLDR's four time formats.

/// How much of a wall-clock time is written: CLDR's four time formats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TimeStyle {
    /// Seconds and the zone's long name: `14:05:00 Midden-Europese zomertijd`.
    Full,
    /// Seconds and the zone's short name: `14:05:00 CEST`.
    Long,
    /// Seconds: `14:05:00`.
    Medium,
    /// Minutes: `14:05`.
    #[default]
    Short,
}
