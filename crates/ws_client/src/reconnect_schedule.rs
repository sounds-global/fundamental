//! [`ReconnectSchedule`] — where a device is in its reconnect schedule.

use std::time::Duration;

use fundamental_core::retry::{Backoff, Entropy, OsEntropy};

use crate::reconnect_hint::ReconnectHint;

/// The widest window any unhinted draw spreads across.
const CAP: Duration = Duration::from_secs(10);

// A rejected `const` schedule fails the build.

/// The series of a device that has never connected: fails fast, retries fast.
const FIRST_CONNECT: Backoff = match Backoff::new(Duration::from_millis(100), 2.0, CAP) {
    Ok(backoff) => backoff,
    Err(_) => panic!("the first-connect schedule is valid"),
};

/// The series after a live session is lost; its first window is the one a whole fleet
/// spreads across after a deploy (gRPC's `INITIAL_BACKOFF`).
const LOST_SESSION: Backoff = match Backoff::new(Duration::from_secs(1), 2.0, CAP) {
    Ok(backoff) => backoff,
    Err(_) => panic!("the lost-session schedule is valid"),
};

/// How long a session must have been live to reset the schedule, so a hub that accepts and
/// drops cannot hold the fleet at the first window.
pub(crate) const STABLE_SESSION: Duration = CAP;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Series {
    FirstConnect,
    LostSession,
}

/// A device's position in the shared full-jitter [`Backoff`]. It has no budget: every
/// ending yields a wait, never a stop.
pub(crate) struct ReconnectSchedule<E = OsEntropy> {
    series: Series,
    step: u32,
    entropy: E,
}

impl ReconnectSchedule<OsEntropy> {
    /// A schedule drawing from the OS CSPRNG.
    pub(crate) fn new() -> Self {
        Self::with_entropy(OsEntropy)
    }
}

impl<E: Entropy> ReconnectSchedule<E> {
    /// A schedule drawing its jitter from `entropy`.
    pub(crate) fn with_entropy(entropy: E) -> Self {
        Self {
            series: Series::FirstConnect,
            step: 0,
            entropy,
        }
    }

    /// The wait before retrying a connect, authentication or subscription that failed.
    pub(crate) fn after_failed_connect(&mut self, hint: Option<ReconnectHint>) -> Duration {
        self.draw(hint)
    }

    /// The wait before redialling a session lost after `lived`, and whether the schedule restarted.
    pub(crate) fn after_lost_session(
        &mut self,
        lived: Duration,
        hint: Option<ReconnectHint>,
    ) -> (Duration, bool) {
        let reset = lived >= STABLE_SESSION || self.series == Series::FirstConnect;
        if reset {
            self.series = Series::LostSession;
            self.step = 0;
        }
        (self.draw(hint), reset)
    }

    /// How many draws the current series has made.
    pub(crate) fn step(&self) -> u32 {
        self.step
    }

    /// The window the next unhinted draw spreads across.
    #[cfg(test)]
    pub(crate) fn window(&self) -> Duration {
        self.backoff().window(self.step)
    }

    fn backoff(&self) -> &'static Backoff {
        match self.series {
            Series::FirstConnect => &FIRST_CONNECT,
            Series::LostSession => &LOST_SESSION,
        }
    }

    fn draw(&mut self, hint: Option<ReconnectHint>) -> Duration {
        let backoff = self.backoff();
        let delay = backoff.draw(self.step, hint.map(ReconnectHint::floor), &mut self.entropy);
        self.step = self.step.saturating_add(1);
        delay
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use fundamental_core::retry::{NoEntropy, SeededEntropy};

    use super::*;
    use crate::reconnect_hint::MAX_HINT_FLOOR;

    fn seeded(seed: u64) -> ReconnectSchedule<SeededEntropy> {
        ReconnectSchedule::with_entropy(SeededEntropy::new(seed))
    }

    /// The fullest 100 ms bucket among `delays`.
    fn largest_bucket(delays: &[Duration]) -> usize {
        fundamental_core::retry::largest_bucket(delays, Duration::from_millis(100))
    }

    /// Every client of a fleet that lost a stable session to the same event.
    fn fleet_first_retries(size: u64, hint: Option<ReconnectHint>) -> Vec<Duration> {
        (0..size)
            .map(|client| seeded(client).after_lost_session(STABLE_SESSION, hint).0)
            .collect()
    }

    #[test]
    fn a_fleet_of_a_thousand_spreads_its_first_retry_across_the_window() {
        let delays = fleet_first_retries(1_000, None);
        assert!(delays.iter().all(|d| *d <= Duration::from_secs(1)));
        let largest = largest_bucket(&delays);
        // Without jitter all 1,000 share one bucket.
        assert!(
            largest <= 130,
            "largest 100 ms bucket held {largest} of 1,000"
        );
    }

    #[test]
    fn a_fleet_of_ten_thousand_spreads_its_first_retry_across_the_window() {
        let largest = largest_bucket(&fleet_first_retries(10_000, None));
        assert!(
            largest <= 1_100,
            "largest 100 ms bucket held {largest} of 10,000"
        );
    }

    #[test]
    fn a_going_away_close_raises_the_floor_and_keeps_the_fleet_spread() {
        let delays = fleet_first_retries(1_000, Some(ReconnectHint::GoingAway));
        assert!(
            delays
                .iter()
                .all(|d| (Duration::from_secs(1)..=Duration::from_secs(2)).contains(d)),
            "1001 waits floor 1 s plus up to 1 s of spread"
        );
        let largest = largest_bucket(&delays);
        assert!(
            largest <= 130,
            "largest 100 ms bucket held {largest} of 1,000"
        );
    }

    #[test]
    fn a_try_again_later_close_raises_the_floor_and_keeps_the_fleet_spread() {
        let delays = fleet_first_retries(1_000, Some(ReconnectHint::TryAgainLater));
        assert!(
            delays
                .iter()
                .all(|d| (Duration::from_secs(5)..=Duration::from_secs(10)).contains(d)),
            "1013 waits floor 5 s plus up to 5 s of spread"
        );
        let largest = largest_bucket(&delays);
        assert!(
            largest <= 40,
            "largest 100 ms bucket held {largest} of 1,000 over 5 s"
        );
    }

    #[test]
    fn no_hint_holds_a_device_beyond_three_hundred_seconds() {
        let ceiling = MAX_HINT_FLOOR * 2;
        assert_eq!(ceiling, Duration::from_secs(300));
        let huge = Some(ReconnectHint::RetryAfter(Duration::from_secs(
            u64::MAX / 2_000,
        )));
        for seed in 0..1_000 {
            let mut schedule = seeded(seed);
            for _ in 0..30 {
                let delay = schedule.after_failed_connect(huge);
                assert!(delay <= ceiling, "waited {delay:?}");
                assert!(delay >= MAX_HINT_FLOOR, "a clamped floor is still a floor");
            }
        }
    }

    #[test]
    fn a_first_connect_walks_the_hundred_millisecond_series_to_the_cap() {
        let mut schedule = ReconnectSchedule::with_entropy(NoEntropy);
        let windows: Vec<u128> = (0..9)
            .map(|_| schedule.after_failed_connect(None).as_millis())
            .collect();
        assert_eq!(
            windows,
            [100, 200, 400, 800, 1_600, 3_200, 6_400, 10_000, 10_000]
        );
    }

    #[test]
    fn a_short_session_advances_the_lost_session_series() {
        let mut schedule = ReconnectSchedule::with_entropy(NoEntropy);
        let short = STABLE_SESSION - Duration::from_millis(1);
        let windows: Vec<u128> = (0..7)
            .map(|_| schedule.after_lost_session(short, None).0.as_millis())
            .collect();
        assert_eq!(
            windows,
            [1_000, 2_000, 4_000, 8_000, 10_000, 10_000, 10_000]
        );
    }

    #[test]
    fn a_failure_after_a_lost_session_continues_its_series() {
        let mut schedule = ReconnectSchedule::with_entropy(NoEntropy);
        for _ in 0..3 {
            schedule.after_failed_connect(None);
        }
        assert_eq!(
            schedule.after_lost_session(STABLE_SESSION, None).0,
            Duration::from_secs(1)
        );
        assert_eq!(schedule.after_failed_connect(None), Duration::from_secs(2));
        assert_eq!(schedule.after_failed_connect(None), Duration::from_secs(4));
    }

    #[test]
    fn a_stable_session_resets_the_schedule() {
        let mut schedule = ReconnectSchedule::with_entropy(NoEntropy);
        for _ in 0..5 {
            schedule.after_lost_session(Duration::ZERO, None);
        }
        assert_eq!(schedule.window(), CAP);
        assert_eq!(
            schedule.after_lost_session(STABLE_SESSION, None).0,
            Duration::from_secs(1)
        );
    }

    #[test]
    fn the_lost_session_reports_whether_it_reset_the_schedule() {
        let mut schedule = ReconnectSchedule::with_entropy(NoEntropy);
        let short = Duration::from_secs(2);
        assert!(
            schedule.after_lost_session(short, None).1,
            "the first loss starts the series"
        );
        assert!(
            !schedule.after_lost_session(short, None).1,
            "a short session advances it"
        );
        assert!(
            schedule.after_lost_session(STABLE_SESSION, None).1,
            "a stable one resets it"
        );
    }

    #[test]
    fn draws_from_the_os_differ() {
        let draws: BTreeSet<_> = (0..64)
            .map(|_| {
                let mut schedule = ReconnectSchedule::new();
                for _ in 0..8 {
                    schedule.after_failed_connect(None);
                }
                schedule.after_failed_connect(None)
            })
            .collect();
        assert!(
            draws.len() > 1,
            "every client drawing one delay is the storm itself"
        );
    }
}
