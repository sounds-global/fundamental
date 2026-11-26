//! [`millis`]: a duration as a log field.

use std::time::Duration;

/// `duration` in whole milliseconds, saturating at `u64::MAX`.
pub(crate) fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_is_counted_in_milliseconds_and_saturates() {
        assert_eq!(millis(Duration::from_millis(1_500)), 1_500);
        assert_eq!(millis(Duration::MAX), u64::MAX);
    }
}
