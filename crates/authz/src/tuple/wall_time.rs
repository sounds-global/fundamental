//! [`WallTime`]: the instant a caveat reads as `now`.

use std::time::{SystemTime, UNIX_EPOCH};

use tracing::error;

/// A reading of real time, as nanoseconds since the Unix epoch.
///
/// A different type from the zookie's [`HybridTs`](crate::zookie::HybridTs), which may be
/// `ZERO`, so a scope cannot hand its zookie instant over as the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WallTime {
    unix_nanos: u64,
}

impl WallTime {
    const NANOS_PER_SECOND: u64 = 1_000_000_000;

    /// The process's wall clock, read now.
    ///
    /// Not through the forward-only [`HybridClock`](crate::zookie::HybridClock), which would keep
    /// a clock step forever. A clock before the epoch reads as the epoch, which is refused.
    #[must_use]
    pub fn now() -> Self {
        let unix_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX))
            .unwrap_or_else(|before_epoch| {
                error!(
                    behind_by = ?before_epoch.duration(),
                    "the system clock reads before the Unix epoch"
                );
                0
            });
        Self { unix_nanos }
    }

    /// A reading of `unix_nanos` nanoseconds since the Unix epoch.
    #[must_use]
    pub const fn from_unix_nanos(unix_nanos: u64) -> Self {
        Self { unix_nanos }
    }

    /// A reading of `unix_seconds` whole seconds since the Unix epoch.
    #[must_use]
    pub const fn from_unix_seconds(unix_seconds: u64) -> Self {
        Self {
            unix_nanos: unix_seconds.saturating_mul(Self::NANOS_PER_SECOND),
        }
    }

    /// Nanoseconds since the Unix epoch.
    #[must_use]
    pub const fn unix_nanos(self) -> u64 {
        self.unix_nanos
    }

    /// Whole seconds since the Unix epoch, floored: the unit of a caveat's
    /// [`Timestamp`](crate::schema::CaveatValue::Timestamp), exact at both edges of a window.
    #[must_use]
    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    pub const fn unix_seconds(self) -> i64 {
        // `u64::MAX / 10^9` is below `i64::MAX`, so the cast cannot wrap.
        #[allow(clippy::cast_possible_wrap)]
        let seconds = (self.unix_nanos / Self::NANOS_PER_SECOND) as i64;
        seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_are_floored_not_rounded() {
        assert_eq!(WallTime::from_unix_nanos(1_999_999_999).unix_seconds(), 1);
        assert_eq!(WallTime::from_unix_nanos(2_000_000_000).unix_seconds(), 2);
    }

    #[test]
    fn the_largest_reading_converts_without_wrapping() {
        assert_eq!(
            WallTime::from_unix_nanos(u64::MAX).unix_seconds(),
            18_446_744_073
        );
    }

    #[test]
    fn whole_seconds_round_trip() {
        assert_eq!(
            WallTime::from_unix_seconds(1_800_000_000).unix_seconds(),
            1_800_000_000
        );
    }

    #[test]
    fn the_process_clock_reads_after_the_floor_the_engine_enforces() {
        assert!(WallTime::now().unix_seconds() > 1_767_225_600);
    }
}
