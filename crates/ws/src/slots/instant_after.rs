//! [`instant_after`]: a deadline that cannot overflow.

use std::time::Duration;

use tokio::time::Instant;
use tracing::error;

use crate::WsConfig;
use crate::observability::millis;

/// The instant `by` after `at`, falling back to [`WsConfig::MAX_BOUND`] rather than overflowing.
pub(crate) fn instant_after(at: Instant, by: Duration) -> Instant {
    at.checked_add(by).unwrap_or_else(|| {
        error!(
            by_ms = millis(by),
            "a deadline overflowed the clock; bounding it to a day"
        );
        at + WsConfig::MAX_BOUND
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bounded_duration_is_added() {
        let at = Instant::now();
        assert_eq!(
            instant_after(at, Duration::from_secs(5)),
            at + Duration::from_secs(5)
        );
    }

    #[test]
    fn an_overflowing_duration_is_bounded_instead_of_panicking() {
        let at = Instant::now();
        assert_eq!(instant_after(at, Duration::MAX), at + WsConfig::MAX_BOUND);
    }
}
