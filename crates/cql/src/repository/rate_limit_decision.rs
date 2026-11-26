//! [`RateLimitDecision`] — the answer a [`RateLimit`](super::RateLimit) gives.

use core::time::Duration;

/// Whether a caller may proceed, and if not, how long to wait.
///
/// [`Shed`](Self::Shed) carries the time until the window ends, for an honest `Retry-After`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitDecision {
    /// The caller is within its allowance.
    Allow {
        /// How many further requests this window still permits. Advisory — a
        /// concurrent caller may take them first.
        remaining: u64,
    },
    /// The caller has exhausted its allowance for this window.
    Shed {
        /// How long until the window rolls and the allowance resets.
        retry_after: Duration,
    },
}

impl RateLimitDecision {
    /// Whether the caller may proceed.
    #[must_use]
    pub const fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }

    /// The retry delay in whole seconds, rounded **up** so a client never retries a
    /// moment early and gets shed a second time.
    ///
    /// `None` when the caller was allowed.
    #[must_use]
    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            Self::Allow { .. } => None,
            // A sub-second remainder still means the window has not rolled, so round
            // up — and never advise `0`, which a client reads as "retry now".
            Self::Shed { retry_after } => Some(retry_after.as_secs().max(1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_reports_no_delay() {
        let decision = RateLimitDecision::Allow { remaining: 4 };
        assert!(decision.is_allowed());
        assert_eq!(decision.retry_after_secs(), None);
    }

    #[test]
    fn shed_rounds_a_sub_second_remainder_up_to_one() {
        let decision = RateLimitDecision::Shed {
            retry_after: Duration::from_millis(120),
        };
        assert!(!decision.is_allowed());
        assert_eq!(
            decision.retry_after_secs(),
            Some(1),
            "advising 0 would tell the client to retry immediately"
        );
    }

    #[test]
    fn shed_reports_whole_seconds() {
        let decision = RateLimitDecision::Shed {
            retry_after: Duration::from_secs(42),
        };
        assert_eq!(decision.retry_after_secs(), Some(42));
    }
}
