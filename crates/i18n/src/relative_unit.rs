//! [`RelativeUnit`] — the unit a relative phrase counts in.

/// The unit a relative phrase counts in: `over 3 dagen`, `2 weeks ago`, `volgende maand`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelativeUnit {
    /// `over 5 minuten`, `this minute`.
    Minute,
    /// `3 uur geleden`, `in 1 hour`.
    Hour,
    /// `vandaag`, `morgen`, `overmorgen`, `over 3 dagen`.
    Day,
    /// `vorige week`, `in 2 weeks`.
    Week,
    /// `volgende maand`, `3 months ago`.
    Month,
    /// `vorig jaar`, `in 2 years`.
    Year,
}

impl RelativeUnit {
    /// Every unit, in the order the relative-word table holds them.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 6] = [
        Self::Minute,
        Self::Hour,
        Self::Day,
        Self::Week,
        Self::Month,
        Self::Year,
    ];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Minute => 0,
            Self::Hour => 1,
            Self::Day => 2,
            Self::Week => 3,
            Self::Month => 4,
            Self::Year => 5,
        }
    }

    /// The unit's ISO 8601 duration designator, and whether it follows the `T`.
    pub(crate) const fn iso_designator(self) -> (char, bool) {
        match self {
            Self::Minute => ('M', true),
            Self::Hour => ('H', true),
            Self::Day => ('D', false),
            Self::Week => ('W', false),
            Self::Month => ('M', false),
            Self::Year => ('Y', false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_unit_has_its_own_index() {
        for (position, unit) in RelativeUnit::ALL.into_iter().enumerate() {
            assert_eq!(unit.index(), position);
        }
    }
}
