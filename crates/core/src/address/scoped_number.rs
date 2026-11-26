//! [`ScopedNumber`] — a record's number inside its scope: `12`.

use core::fmt;
use core::num::NonZeroU64;

#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};

use crate::address::PathSegment;
use crate::model::Spelled;

/// A record's number inside its scope — the twelfth request of a team is `12` — assigned
/// gaplessly. It reveals a count only inside its own scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopedNumber(NonZeroU64);

impl ScopedNumber {
    /// The largest number the store holds (`INT8`).
    pub const MAX: u64 = i64::MAX as u64;

    /// The number `value`, when it is 1 to [`MAX`](Self::MAX).
    #[must_use]
    pub fn new(value: u64) -> Option<Self> {
        (value <= Self::MAX)
            .then(|| NonZeroU64::new(value))
            .flatten()
            .map(Self)
    }

    /// Read the one spelling: digits, no leading zero.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match Self::read(text)? {
            Spelled::Canonical(number) => Some(number),
            Spelled::Variant(_) => None,
        }
    }

    /// Read a number with leading zeros as a variant of its one spelling.
    #[must_use]
    pub fn read(text: &str) -> Option<Spelled<Self>> {
        if text.is_empty() || text.len() > 64 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let number = Self::new(text.trim_start_matches('0').parse().ok()?)?;
        Some(match text.starts_with('0') {
            true => Spelled::Variant(number),
            false => Spelled::Canonical(number),
        })
    }

    /// The number.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0.get()
    }

    /// The number as a store's `INT8` holds it.
    #[must_use]
    pub fn stored(self) -> i64 {
        i64::try_from(self.0.get()).unwrap_or(i64::MAX)
    }
}

impl PathSegment for ScopedNumber {
    fn to_segment(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for ScopedNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::model::Spelled;

    use super::ScopedNumber;

    #[test]
    fn leading_zeros_are_a_variant_and_zero_is_no_number() {
        let twelve = ScopedNumber::new(12);
        assert_eq!(ScopedNumber::read("12"), twelve.map(Spelled::Canonical));
        assert_eq!(ScopedNumber::read("012"), twelve.map(Spelled::Variant));
        assert_eq!(ScopedNumber::parse("012"), None);
        for refused in ["0", "000", "", "-1", "1a", "99999999999999999999"] {
            assert_eq!(ScopedNumber::read(refused), None, "{refused}");
        }
        assert_eq!(
            ScopedNumber::new(ScopedNumber::MAX).map(ScopedNumber::stored),
            Some(i64::MAX)
        );
    }
}
