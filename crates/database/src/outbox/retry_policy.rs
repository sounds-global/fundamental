//! [`RetryPolicy`]: how long a claimed row stays invisible, and how long a failed one waits.

use core::time::Duration;

use chrono::{DateTime, SubsecRound, TimeDelta, Utc};

/// The lease and backoff schedule the outbox drain applies.
///
/// The lease and the backoff both write the row's next-attempt time. The backoff doubles from
/// [`base`](Self::base) up to [`max`](Self::max), less a jitter of up to 25% derived from the
/// row's `msg_id`, so a failing cohort does not retry in lockstep.
/// [`with_recurrence`](Self::with_recurrence) declares a recurring row's period, which
/// [`deferral_horizon`](Self::deferral_horizon) then covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    lease: Duration,
    base: Duration,
    max: Duration,
    recurrence: Option<Duration>,
}

impl RetryPolicy {
    /// Builds a policy from a claim lease and a backoff range.
    ///
    /// The lease should comfortably exceed one relay; the default is 30 s.
    #[must_use]
    pub const fn new(lease: Duration, base: Duration, max: Duration) -> Self {
        Self {
            lease,
            base,
            max,
            recurrence: None,
        }
    }

    /// Declares that the relay re-schedules a row it relayed `period` ahead rather than
    /// finishing it.
    #[must_use]
    pub const fn with_recurrence(mut self, period: Duration) -> Self {
        self.recurrence = Some(period);
        self
    }

    /// How long a claimed row stays invisible to other workers.
    #[must_use]
    pub const fn lease(&self) -> Duration {
        self.lease
    }

    /// The delay after the first failure, doubled on each subsequent one.
    #[must_use]
    pub const fn base(&self) -> Duration {
        self.base
    }

    /// The ceiling on the backoff delay.
    #[must_use]
    pub const fn max(&self) -> Duration {
        self.max
    }

    /// The period a recurring row is re-scheduled by, when the table recurs.
    #[must_use]
    pub const fn recurrence(&self) -> Option<Duration> {
        self.recurrence
    }

    /// How far past a row's claim anything this drain writes can schedule it: one lease, then
    /// the longer of the longest backoff and the declared recurrence.
    ///
    ///
    /// `None` when that span is not representable.
    #[must_use]
    pub fn deferral_horizon(&self) -> Option<Duration> {
        let furthest = match self.recurrence {
            Some(period) if period > self.max => period,
            _ => self.max,
        };
        self.lease.checked_add(furthest)
    }

    /// When a row claimed at `now` becomes available again if this worker never
    /// finishes it.
    ///
    /// Whole microseconds and at least one past `now`, so a later claim always writes a strictly
    /// later lease.
    #[must_use]
    pub fn lease_until(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        let one_micro = TimeDelta::microseconds(1);
        let floor = now
            .trunc_subsecs(6)
            .checked_add_signed(one_micro)
            .unwrap_or(DateTime::<Utc>::MAX_UTC);
        now.checked_add_signed(delta(self.lease))
            .map_or(DateTime::<Utc>::MAX_UTC, |until| until.trunc_subsecs(6))
            .max(floor)
    }

    /// When a row that has now failed `attempts` times should next be attempted.
    ///
    ///
    /// `attempts` includes the failure just recorded; a non-positive count is treated as one.
    #[must_use]
    pub fn retry_at(&self, attempts: i64, msg_id: &str, now: DateTime<Utc>) -> DateTime<Utc> {
        now + delta(self.delay_for(attempts, msg_id))
    }

    /// The backoff delay for `attempts`, jittered for `msg_id`.
    #[must_use]
    pub fn delay_for(&self, attempts: i64, msg_id: &str) -> Duration {
        // Clamped: a shift by 63+ is undefined for u64.
        let doublings = u32::try_from(attempts.max(1) - 1)
            .unwrap_or(u32::MAX)
            .min(32);
        let base_ms = u64::try_from(self.base.as_millis()).unwrap_or(u64::MAX);
        let max_ms = u64::try_from(self.max.as_millis()).unwrap_or(u64::MAX);
        let capped = base_ms
            .checked_shl(doublings)
            .unwrap_or(u64::MAX)
            .min(max_ms);

        // Keep at least 75% of the delay and shave up to 25% per row.
        let spread = capped / 4;
        let shaved = if spread == 0 {
            0
        } else {
            fnv1a(msg_id.as_bytes()) % spread
        };
        Duration::from_millis(capped - shaved)
    }
}

impl Default for RetryPolicy {
    /// A 30-second claim lease, backing off from 5 seconds to 15 minutes.
    fn default() -> Self {
        Self::new(
            Duration::from_secs(30),
            Duration::from_secs(5),
            Duration::from_secs(15 * 60),
        )
    }
}

/// Saturates, because a duration too large to represent means "effectively never".
fn delta(duration: Duration) -> TimeDelta {
    TimeDelta::from_std(duration).unwrap_or(TimeDelta::MAX)
}

/// FNV-1a, because the jitter must be reproducible across processes and Rust versions.
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RetryPolicy {
        RetryPolicy::new(
            Duration::from_secs(30),
            Duration::from_secs(4),
            Duration::from_secs(60),
        )
    }

    #[test]
    fn the_delay_doubles_per_attempt() {
        let policy = policy();
        // Jitter shaves up to 25%, so each attempt is asserted as a band.
        for (attempts, nominal) in [(1, 4), (2, 8), (3, 16), (4, 32)] {
            let delay = policy.delay_for(attempts, "row-a");
            assert!(
                delay <= Duration::from_secs(nominal)
                    && delay >= Duration::from_millis(nominal * 750),
                "attempt {attempts} should land in [75%, 100%] of {nominal}s, got {delay:?}"
            );
        }
    }

    #[test]
    fn the_delay_is_capped() {
        let policy = policy();
        for attempts in [5, 20, i64::MAX] {
            let delay = policy.delay_for(attempts, "row-a");
            assert!(
                delay <= Duration::from_secs(60),
                "attempt {attempts} must not exceed the cap, got {delay:?}"
            );
        }
    }

    #[test]
    fn a_non_positive_attempt_count_still_backs_off() {
        let policy = policy();
        for attempts in [0, -1, i64::MIN] {
            assert!(
                policy.delay_for(attempts, "row-a") > Duration::ZERO,
                "attempt {attempts} must never mean 'retry immediately'"
            );
        }
    }

    #[test]
    fn jitter_decorrelates_rows_that_failed_together() {
        let policy = policy();
        let first = policy.delay_for(3, "row-a");
        let second = policy.delay_for(3, "row-b");
        assert_ne!(
            first, second,
            "two rows failing on the same tick must not retry in lockstep"
        );
    }

    #[test]
    fn jitter_is_reproducible_for_the_same_row() {
        let policy = policy();
        assert_eq!(
            policy.delay_for(3, "row-a"),
            policy.delay_for(3, "row-a"),
            "the schedule must be stable, not random"
        );
    }

    #[test]
    fn a_claim_lease_and_a_backoff_both_move_the_row_into_the_future() {
        let policy = policy();
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("valid timestamp");

        assert_eq!(policy.lease_until(now), now + TimeDelta::seconds(30));

        let retry = policy.retry_at(1, "row-a", now);
        assert!(
            retry > now,
            "a failed row must not be immediately claimable"
        );
        assert!(retry <= now + TimeDelta::seconds(4));
    }

    #[test]
    fn a_lease_is_whole_microseconds_so_the_stored_token_compares_equal() {
        let now =
            DateTime::<Utc>::from_timestamp(1_700_000_000, 123_456_789).expect("valid timestamp");
        let until = policy().lease_until(now);
        assert_eq!(until.timestamp_subsec_nanos() % 1_000, 0);
        assert_eq!(until, now.trunc_subsecs(6) + TimeDelta::seconds(30));
    }

    #[test]
    fn a_zero_lease_still_moves_the_token_past_now() {
        let now =
            DateTime::<Utc>::from_timestamp(1_700_000_000, 999_999_999).expect("valid timestamp");
        let zero = RetryPolicy::new(
            Duration::ZERO,
            Duration::from_secs(1),
            Duration::from_secs(2),
        );
        let until = zero.lease_until(now);
        assert!(
            until > now,
            "a later claim must always write a strictly later token"
        );
    }

    #[test]
    fn the_default_policy_is_a_short_lease_and_a_bounded_backoff() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.lease(), Duration::from_secs(30));
        assert_eq!(policy.base(), Duration::from_secs(5));
        assert_eq!(policy.max(), Duration::from_secs(900));
        assert_eq!(policy.recurrence(), None);
    }

    #[test]
    fn a_policy_that_does_not_recur_defers_no_further_than_a_lease_and_the_longest_backoff() {
        assert_eq!(policy().deferral_horizon(), Some(Duration::from_secs(90)));
    }

    #[test]
    fn a_declared_recurrence_longer_than_the_backoff_sets_the_horizon() {
        let recurring = policy().with_recurrence(Duration::from_secs(3 * 3600));
        assert_eq!(recurring.recurrence(), Some(Duration::from_secs(3 * 3600)));
        assert_eq!(
            recurring.deferral_horizon(),
            Some(Duration::from_secs(3 * 3600 + 30))
        );
    }

    #[test]
    fn a_recurrence_shorter_than_the_backoff_leaves_the_backoff_as_the_horizon() {
        let recurring = policy().with_recurrence(Duration::from_secs(10));
        assert_eq!(recurring.deferral_horizon(), Some(Duration::from_secs(90)));
    }

    #[test]
    fn an_unrepresentable_horizon_is_none_rather_than_a_wrapped_span() {
        let recurring = policy().with_recurrence(Duration::MAX);
        assert_eq!(recurring.deferral_horizon(), None);
    }

    #[test]
    fn a_recurrence_changes_no_retry_the_policy_schedules() {
        let recurring = policy().with_recurrence(Duration::from_secs(3 * 3600));
        for attempts in [1, 3, 20] {
            assert_eq!(
                recurring.delay_for(attempts, "row-a"),
                policy().delay_for(attempts, "row-a")
            );
        }
    }
}
