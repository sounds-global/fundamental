//! [`Backoff`] — the full-jitter schedule every reconnecting client draws from.

use core::future::Future;
use core::num::NonZeroU32;
use core::time::Duration;

use tracing::{error, warn};

use super::{BackoffRejection, Entropy, OsEntropy};
use crate::error::Error;

/// A full-jitter schedule: the wait at `step` is uniform in `[0, window(step)]`, the window
/// growing geometrically from `first` up to `cap`.
///
/// Full rather than equal jitter, because equal jitter doubles a fleet's peak arrival rate.
///
/// ```
/// use core::time::Duration;
/// use fundamental_core::retry::{Backoff, OsEntropy};
///
/// let backoff = Backoff::new(Duration::from_secs(1), 2.0, Duration::from_secs(10))?;
/// assert_eq!(backoff.window(3), Duration::from_secs(8));
/// assert!(backoff.draw(3, None, &mut OsEntropy) <= Duration::from_secs(8));
/// # Ok::<(), fundamental_core::retry::BackoffRejection>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Backoff {
    first: Duration,
    growth: f64,
    cap: Duration,
}

impl Backoff {
    /// The shortest first window a schedule may have.
    pub const MIN_FIRST: Duration = Duration::from_millis(1);

    /// A schedule whose window starts at `first`, grows by `growth` per step, and stops at `cap`.
    ///
    /// # Errors
    ///
    /// [`BackoffRejection`] when the schedule could wait zero or shrink towards it.
    pub const fn new(
        first: Duration,
        growth: f64,
        cap: Duration,
    ) -> Result<Self, BackoffRejection> {
        if first.as_nanos() < Self::MIN_FIRST.as_nanos() {
            return Err(BackoffRejection::FirstTooShort { first });
        }
        // `is_finite` first: NaN compares false with everything.
        if !growth.is_finite() || growth < 1.0 {
            return Err(BackoffRejection::GrowthBelowOne { growth });
        }
        if cap.as_nanos() < first.as_nanos() {
            return Err(BackoffRejection::CapBelowFirst { first, cap });
        }
        Ok(Self { first, growth, cap })
    }

    /// The first window.
    #[must_use]
    pub const fn first(&self) -> Duration {
        self.first
    }

    /// The widest window any unhinted draw spreads across.
    #[must_use]
    pub const fn cap(&self) -> Duration {
        self.cap
    }

    /// The window the draw at `step` spreads across: `first × growth^step`, capped.
    #[must_use]
    pub fn window(&self, step: u32) -> Duration {
        let exponent = i32::try_from(step).unwrap_or(i32::MAX);
        let scaled_nanos = self.first.as_nanos() as f64 * self.growth.powi(exponent);
        let cap_nanos = self.cap.as_nanos() as f64;
        if scaled_nanos < cap_nanos {
            // Below the cap, so it fits in `u64` nanoseconds; rounding keeps a power-of-two growth exact.
            Duration::from_nanos(scaled_nanos.round() as u64)
        } else {
            self.cap
        }
    }

    /// The wait at `step`: uniform in `[0, window(step)]`, or `floor + uniform[0, w]` with
    /// `w = max(window(step), floor)` when a server gave a floor; an empty source gives the upper bound.
    #[must_use]
    pub fn draw(&self, step: u32, floor: Option<Duration>, entropy: &mut impl Entropy) -> Duration {
        let window = self.window(step);
        match floor {
            None => uniform(window, entropy),
            Some(floor) => floor.saturating_add(uniform(window.max(floor), entropy)),
        }
    }

    /// Runs `op` until it succeeds, fails non-retryably, or has been tried `attempts` times.
    ///
    /// The error's own [`retry_after_secs`](Error::retry_after_secs) is the draw's floor.
    ///
    /// ```
    /// use core::num::NonZeroU32;
    /// use core::time::Duration;
    /// use fundamental_core::retry::Backoff;
    /// # use std::borrow::Cow;
    /// # #[derive(Debug)]
    /// # struct Busy;
    /// # impl core::fmt::Display for Busy {
    /// #     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str("busy") }
    /// # }
    /// # impl core::error::Error for Busy {}
    /// # impl fundamental_core::error::Error for Busy {
    /// #     fn error_code(&self) -> &'static str { "DOC_001" }
    /// #     fn display_message(&self) -> Cow<'_, str> { Cow::Borrowed("busy") }
    /// #     fn is_retryable(&self) -> bool { true }
    /// # }
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let backoff = Backoff::new(Duration::from_millis(1), 2.0, Duration::from_millis(4))?;
    /// let three = NonZeroU32::new(3).ok_or("three is not zero")?;
    /// let mut calls = 0;
    /// let answer = backoff
    ///     .retry("doc", three, |_| async {}, || {
    ///         calls += 1;
    ///         let ready = calls == 3;
    ///         async move { if ready { Ok(42) } else { Err(Busy) } }
    ///     })
    ///     .await;
    /// assert_eq!(answer.ok(), Some(42));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// The last error `op` returned: the first that is not retryable, or the final attempt's.
    pub async fn retry<T, E, Op, Fut, Sleep, Slept>(
        &self,
        operation: &'static str,
        attempts: NonZeroU32,
        sleep: Sleep,
        op: Op,
    ) -> Result<T, E>
    where
        E: Error,
        Op: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        Sleep: FnMut(Duration) -> Slept,
        Slept: Future<Output = ()>,
    {
        self.retry_drawing(operation, attempts, &mut OsEntropy, sleep, op)
            .await
    }

    /// [`retry`](Self::retry) drawing from `entropy`, so a test knows every wait.
    #[expect(
        clippy::cognitive_complexity,
        clippy::too_many_arguments,
        reason = "readability baseline"
    )]
    async fn retry_drawing<T, E, Op, Fut, Sleep, Slept>(
        &self,
        operation: &'static str,
        attempts: NonZeroU32,
        entropy: &mut impl Entropy,
        mut sleep: Sleep,
        mut op: Op,
    ) -> Result<T, E>
    where
        E: Error,
        Op: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        Sleep: FnMut(Duration) -> Slept,
        Slept: Future<Output = ()>,
    {
        let mut attempt = 1;
        loop {
            let failure = match op().await {
                Ok(value) => return Ok(value),
                Err(failure) => failure,
            };
            if !failure.is_retryable() {
                warn!(
                    operation,
                    attempt,
                    error_code = failure.error_code(),
                    error = %failure,
                    "not retryable; giving up at once"
                );
                return Err(failure);
            }
            if attempt >= attempts.get() {
                error!(
                    operation,
                    attempts = attempts.get(),
                    error_code = failure.error_code(),
                    error = %failure,
                    "every attempt failed"
                );
                return Err(failure);
            }
            let floor = failure.retry_after_secs().map(Duration::from_secs);
            let delay = self.draw(attempt - 1, floor, entropy);
            warn!(
                operation,
                attempt,
                error_code = failure.error_code(),
                error = %failure,
                delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                "retrying after a backoff"
            );
            sleep(delay).await;
            attempt += 1;
        }
    }
}

/// A uniform draw in `[0, max]` at millisecond resolution, or `max` when the source is empty.
fn uniform(max: Duration, entropy: &mut impl Entropy) -> Duration {
    let max_ms = u64::try_from(max.as_millis()).unwrap_or(u64::MAX);
    let drawn = match entropy.next_u64() {
        // Multiply-shift: unbiased to within (max_ms + 1) / 2^64, with no `%`.
        Some(word) => {
            let scaled = (u128::from(word) * (u128::from(max_ms) + 1)) >> 64;
            u64::try_from(scaled).unwrap_or(max_ms)
        }
        None => max_ms,
    };
    Duration::from_millis(drawn)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::cell::RefCell;
    use std::fmt;

    use super::*;
    use crate::retry::{NoEntropy, SeededEntropy, largest_bucket};

    #[derive(Debug)]
    struct Failure {
        retryable: bool,
        after: Option<u64>,
    }

    impl fmt::Display for Failure {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("planted failure")
        }
    }

    impl core::error::Error for Failure {}

    impl Error for Failure {
        fn error_code(&self) -> &'static str {
            "TEST_001"
        }

        fn display_message(&self) -> Cow<'_, str> {
            Cow::Borrowed("planted failure")
        }

        fn is_retryable(&self) -> bool {
            self.retryable
        }

        fn retry_after_secs(&self) -> Option<u64> {
            self.after
        }
    }

    const BUSY: Failure = Failure {
        retryable: true,
        after: None,
    };

    fn attempts(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).unwrap_or(NonZeroU32::MIN)
    }

    /// Runs `retry_drawing` with no entropy; returns the outcome, the call count and every wait.
    fn run(
        attempts: NonZeroU32,
        mut outcomes: Vec<Result<u32, Failure>>,
    ) -> (Result<u32, Failure>, usize, Vec<Duration>) {
        let slept = RefCell::new(Vec::new());
        let mut calls = 0;
        outcomes.reverse();
        let outcome = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap_or_else(|e| panic!("a test runtime: {e}"))
            .block_on(doubling().retry_drawing(
                "test",
                attempts,
                &mut NoEntropy,
                |delay| {
                    slept.borrow_mut().push(delay);
                    async {}
                },
                || {
                    calls += 1;
                    let next = outcomes.pop().unwrap_or(Err(BUSY));
                    async move { next }
                },
            ));
        (outcome, calls, slept.into_inner())
    }

    #[test]
    fn retry_returns_the_first_success_without_waiting() {
        let (outcome, calls, slept) = run(attempts(5), vec![Ok(7)]);
        assert_eq!(outcome.ok(), Some(7));
        assert_eq!(calls, 1);
        assert!(slept.is_empty());
    }

    #[test]
    fn retry_sleeps_each_window_between_attempts() {
        let (outcome, calls, slept) = run(attempts(5), vec![Err(BUSY), Err(BUSY), Ok(9)]);
        assert_eq!(outcome.ok(), Some(9));
        assert_eq!(calls, 3);
        assert_eq!(
            slept,
            [Duration::from_millis(100), Duration::from_millis(200)]
        );
    }

    #[test]
    fn retry_stops_at_an_error_that_is_not_retryable_without_sleeping() {
        let fatal = Failure {
            retryable: false,
            after: None,
        };
        let (outcome, calls, slept) = run(attempts(5), vec![Err(fatal), Ok(1)]);
        assert!(matches!(
            outcome,
            Err(Failure {
                retryable: false,
                ..
            })
        ));
        assert_eq!(calls, 1);
        assert!(slept.is_empty());
    }

    #[test]
    fn retry_gives_back_the_last_error_after_its_attempts() {
        let (outcome, calls, slept) = run(attempts(3), Vec::new());
        assert!(matches!(
            outcome,
            Err(Failure {
                retryable: true,
                ..
            })
        ));
        assert_eq!(calls, 3);
        assert_eq!(slept.len(), 2, "no wait after the last attempt");
    }

    #[test]
    fn retry_waits_at_least_what_the_error_asked() {
        let throttled = Failure {
            retryable: true,
            after: Some(3),
        };
        let (_, _, slept) = run(attempts(2), vec![Err(throttled), Ok(1)]);
        assert_eq!(slept, [Duration::from_secs(6)]);
    }

    fn doubling() -> Backoff {
        Backoff::new(Duration::from_millis(100), 2.0, Duration::from_secs(10))
            .unwrap_or_else(|rejection| panic!("a valid schedule was refused: {rejection}"))
    }

    #[test]
    fn the_window_grows_geometrically_to_the_cap() {
        let backoff = doubling();
        let windows: Vec<u128> = (0..9)
            .map(|step| backoff.window(step).as_millis())
            .collect();
        assert_eq!(
            windows,
            [100, 200, 400, 800, 1_600, 3_200, 6_400, 10_000, 10_000]
        );
        assert_eq!(backoff.window(u32::MAX), Duration::from_secs(10));
    }

    #[test]
    fn a_fractional_growth_follows_grpc() {
        let grpc = Backoff::new(Duration::from_secs(1), 1.6, Duration::from_secs(120))
            .unwrap_or_else(|rejection| panic!("{rejection}"));
        assert_eq!(grpc.window(1), Duration::from_millis(1_600));
        assert_eq!(grpc.window(2), Duration::from_millis(2_560));
        assert_eq!(grpc.window(50), Duration::from_secs(120));
    }

    #[test]
    fn full_jitter_stays_within_its_window_over_ten_thousand_draws() {
        let backoff = doubling();
        let window = Duration::from_millis(6_400);
        let draws: Vec<Duration> = (0..10_000u64)
            .map(|seed| backoff.draw(6, None, &mut SeededEntropy::new(seed)))
            .collect();
        assert!(
            draws.iter().all(|d| *d <= window),
            "a draw left [0, window]"
        );
        let mean_ms = draws.iter().map(Duration::as_millis).sum::<u128>() / draws.len() as u128;
        assert!(
            (3_100..=3_300).contains(&mean_ms),
            "uniform over [0, 6400] means ~3200 ms, got {mean_ms}"
        );
        let low = draws.iter().filter(|d| **d < window / 10).count();
        let high = draws.iter().filter(|d| **d > window * 9 / 10).count();
        assert!(
            (900..=1_100).contains(&low) && (900..=1_100).contains(&high),
            "each tenth of the window holds ~1000 draws: low {low}, high {high}"
        );
    }

    #[test]
    fn a_fleet_of_a_thousand_spreads_across_the_window() {
        let backoff = Backoff::new(Duration::from_secs(1), 2.0, Duration::from_secs(10))
            .unwrap_or_else(|rejection| panic!("{rejection}"));
        let delays: Vec<Duration> = (0..1_000u64)
            .map(|seed| backoff.draw(0, None, &mut SeededEntropy::new(seed)))
            .collect();
        let largest = largest_bucket(&delays, Duration::from_millis(100));
        assert!(
            largest <= 130,
            "largest 100 ms bucket held {largest} of 1,000"
        );
    }

    #[test]
    fn a_floor_raises_the_draw_and_keeps_the_spread_at_least_as_wide() {
        let backoff = doubling();
        let floor = Duration::from_secs(5);
        for seed in 0..1_000 {
            let drawn = backoff.draw(0, Some(floor), &mut SeededEntropy::new(seed));
            assert!((floor..=floor * 2).contains(&drawn), "drew {drawn:?}");
        }
        let zero_floor = backoff.draw(3, Some(Duration::ZERO), &mut SeededEntropy::new(9));
        assert_eq!(
            zero_floor,
            backoff.draw(3, None, &mut SeededEntropy::new(9))
        );
    }

    #[test]
    fn without_entropy_the_draw_is_the_upper_bound_never_zero() {
        let backoff = doubling();
        assert_eq!(
            backoff.draw(0, None, &mut NoEntropy),
            Duration::from_millis(100)
        );
        assert_eq!(
            backoff.draw(0, Some(Duration::from_secs(1)), &mut NoEntropy),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn a_zero_width_schedule_is_refused() {
        let cap = Duration::from_secs(10);
        assert_eq!(
            Backoff::new(Duration::ZERO, 2.0, cap),
            Err(BackoffRejection::FirstTooShort {
                first: Duration::ZERO
            })
        );
        assert!(matches!(
            Backoff::new(Duration::from_micros(999), 2.0, cap),
            Err(BackoffRejection::FirstTooShort { .. })
        ));
        for growth in [0.0, 0.5, 0.999, -2.0, f64::NAN, f64::INFINITY] {
            assert!(
                matches!(
                    Backoff::new(Duration::from_millis(100), growth, cap),
                    Err(BackoffRejection::GrowthBelowOne { .. })
                ),
                "growth {growth} was accepted"
            );
        }
        assert_eq!(
            Backoff::new(Duration::from_secs(1), 2.0, Duration::ZERO),
            Err(BackoffRejection::CapBelowFirst {
                first: Duration::from_secs(1),
                cap: Duration::ZERO
            })
        );
        assert!(Backoff::new(Backoff::MIN_FIRST, 1.0, Backoff::MIN_FIRST).is_ok());
    }

    #[test]
    fn an_accepted_schedule_never_has_a_zero_window() {
        let tightest = Backoff::new(Backoff::MIN_FIRST, 1.0, Backoff::MIN_FIRST)
            .unwrap_or_else(|rejection| panic!("{rejection}"));
        for step in [0, 1, 1_000, u32::MAX] {
            assert_eq!(tightest.window(step), Backoff::MIN_FIRST);
        }
    }

    #[test]
    fn the_rejection_names_the_parameter() {
        let message = BackoffRejection::GrowthBelowOne { growth: 0.5 }.to_string();
        assert!(message.contains("growth is 0.5"), "{message}");
    }
}
