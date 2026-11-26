//! [`HybridTs`]: a portable hybrid logical timestamp for ordering writes.

use core::fmt;

use serde::{Deserialize, Serialize};

/// A hybrid logical timestamp: a wall-clock component (nanoseconds since the Unix
/// epoch) plus a logical counter that breaks ties within the same nanosecond.
///
///
/// The backend-neutral, comparable form of a commit timestamp, ordered by `(wall, logical)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HybridTs {
    /// Nanoseconds since the Unix epoch.
    pub wall: u64,
    /// Logical tiebreaker within the same wall nanosecond.
    pub logical: u32,
}

impl HybridTs {
    /// The zero timestamp (older than any real write).
    pub const ZERO: Self = Self {
        wall: 0,
        logical: 0,
    };

    /// Builds a timestamp from its parts.
    #[must_use]
    pub const fn new(wall: u64, logical: u32) -> Self {
        Self { wall, logical }
    }

    /// Parses a hybrid-logical-clock decimal, `{wall}` or `{wall}.{logical}`.
    ///
    /// # Errors
    ///
    /// Returns `None` if `hlc` is not a bare decimal.
    #[must_use]
    pub fn from_hlc(hlc: &str) -> Option<Self> {
        let mut parts = hlc.splitn(2, '.');
        let wall = parts.next()?.parse::<u64>().ok()?;
        let logical = match parts.next() {
            None => 0,
            Some(frac) => frac.parse::<u32>().ok()?,
        };
        Some(Self { wall, logical })
    }
}

impl fmt::Display for HybridTs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.wall, self.logical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_by_wall_then_logical() {
        assert!(HybridTs::new(1, 0) < HybridTs::new(2, 0));
        assert!(HybridTs::new(1, 0) < HybridTs::new(1, 1));
        assert!(HybridTs::new(2, 0) > HybridTs::new(1, 9));
        assert_eq!(HybridTs::new(5, 3), HybridTs::new(5, 3));
    }

    #[test]
    fn parses_hlc_with_and_without_logical() {
        assert_eq!(
            HybridTs::from_hlc("1700000000000000000"),
            Some(HybridTs::new(1_700_000_000_000_000_000, 0))
        );
        assert_eq!(
            HybridTs::from_hlc("1700000000000000000.42"),
            Some(HybridTs::new(1_700_000_000_000_000_000, 42))
        );
    }

    #[test]
    fn rejects_non_decimal_hlc() {
        assert_eq!(HybridTs::from_hlc("now()"), None);
        assert_eq!(HybridTs::from_hlc("1; DROP TABLE x"), None);
        assert_eq!(HybridTs::from_hlc(""), None);
    }
}
