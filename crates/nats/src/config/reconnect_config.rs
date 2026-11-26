use std::time::Duration;

use fundamental_core::retry::Backoff;
use serde::{Deserialize, Serialize};

use crate::{NatsError, NatsResult};

/// How a client waits between reconnects: the shared full-jitter
/// [`Backoff`](fundamental_core::retry::Backoff), whose window starts at
/// [`initial_delay`](Self::initial_delay), grows by
/// [`backoff_multiplier`](Self::backoff_multiplier) per attempt and stops at
/// [`max_delay`](Self::max_delay).
///
/// A client never gives up on its own, since a stop state lets anyone who resets handshakes
/// take a service down. A removed setting fails to load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconnectConfig {
    /// Whether a failed first connect is retried (`true`) or returned at once (`false`); a lost
    /// connection is always retried.
    #[serde(default = "default_true")]
    pub retry_initial_connect: bool,
    /// The first window, at least [`MIN_INITIAL_DELAY`](Self::MIN_INITIAL_DELAY).
    #[serde(default = "default_initial_delay", with = "super::duration_serde")]
    pub initial_delay: Duration,
    /// The widest window, at least [`MIN_MAX_DELAY`](Self::MIN_MAX_DELAY) and at least
    /// [`initial_delay`](Self::initial_delay).
    #[serde(default = "default_max_delay", with = "super::duration_serde")]
    pub max_delay: Duration,
    /// How much the window grows per attempt: a finite number of at least 1.
    #[serde(default = "default_backoff_multiplier")]
    pub backoff_multiplier: f64,
}

fn default_true() -> bool {
    true
}

fn default_initial_delay() -> Duration {
    Duration::from_millis(100)
}

fn default_max_delay() -> Duration {
    Duration::from_secs(30)
}

fn default_backoff_multiplier() -> f64 {
    2.0
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            retry_initial_connect: true,
            initial_delay: default_initial_delay(),
            max_delay: default_max_delay(),
            backoff_multiplier: default_backoff_multiplier(),
        }
    }
}

impl ReconnectConfig {
    /// The shortest first window a reconnect may have, so a recovering server is not stormed.
    pub const MIN_INITIAL_DELAY: Duration = Duration::from_millis(50);

    /// The shortest widest window, bounding a failing client at about four attempts a second.
    pub const MIN_MAX_DELAY: Duration = Duration::from_millis(500);

    /// A policy whose first connect fails at once, for a probe, a CLI or a test.
    #[must_use]
    pub fn fail_fast() -> Self {
        Self {
            retry_initial_connect: false,
            ..Default::default()
        }
    }

    /// Sets the first window.
    #[must_use]
    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    /// Sets the widest window.
    #[must_use]
    pub fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Sets how much the window grows per attempt.
    #[must_use]
    pub fn with_backoff_multiplier(mut self, multiplier: f64) -> Self {
        self.backoff_multiplier = multiplier;
        self
    }

    /// The schedule these settings describe.
    ///
    /// # Errors
    ///
    /// [`NatsError::Config`] naming the settings when `initial_delay` is under
    /// [`MIN_INITIAL_DELAY`](Self::MIN_INITIAL_DELAY), `max_delay` is under
    /// [`MIN_MAX_DELAY`](Self::MIN_MAX_DELAY) or `initial_delay`, or `backoff_multiplier` is not a
    /// finite number of at least 1.
    pub fn backoff(&self) -> NatsResult<Backoff> {
        if self.initial_delay < Self::MIN_INITIAL_DELAY || self.max_delay < Self::MIN_MAX_DELAY {
            return Err(NatsError::Config(format!(
                "reconnect initial_delay {:?} and max_delay {:?} must be at least {:?} and {:?}, \
                 or every client redials in a storm",
                self.initial_delay,
                self.max_delay,
                Self::MIN_INITIAL_DELAY,
                Self::MIN_MAX_DELAY
            )));
        }
        Backoff::new(self.initial_delay, self.backoff_multiplier, self.max_delay).map_err(
            |rejection| {
                NatsError::Config(format!(
                    "reconnect settings initial_delay, backoff_multiplier and max_delay are \
                     unusable: {rejection}"
                ))
            },
        )
    }
}

/// The step of the shared schedule a driver's reconnect `attempt` draws from; async-nats counts
/// from 1.
pub(crate) fn step_for_attempt(attempt: usize) -> u32 {
    u32::try_from(attempt.saturating_sub(1)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;
    use fundamental_core::retry::{SeededEntropy, largest_bucket};

    use super::*;

    /// Equal jitter, the baseline full jitter is compared against.
    fn equal_jitter(window: Duration, entropy: &mut SeededEntropy) -> Duration {
        use fundamental_core::retry::Entropy;
        let millis = u64::try_from(window.as_millis()).unwrap_or(u64::MAX);
        let half = millis / 2;
        let word = entropy.next_u64().unwrap_or(0);
        Duration::from_millis(half + word % (millis - half + 1))
    }

    #[test]
    fn a_fleet_of_a_thousand_halves_its_peak_under_full_jitter() {
        // Attempt 4 of the default schedule: a 800 ms window, eight 100 ms buckets.
        let config = ReconnectConfig::default();
        let backoff = config.backoff().unwrap_or_else(|error| panic!("{error}"));
        let step = step_for_attempt(4);
        let window = backoff.window(step);
        assert_eq!(window, Duration::from_millis(800));

        let full: Vec<Duration> = (0..1_000u64)
            .map(|seed| backoff.draw(step, None, &mut SeededEntropy::new(seed)))
            .collect();
        let equal: Vec<Duration> = (0..1_000u64)
            .map(|seed| equal_jitter(window, &mut SeededEntropy::new(seed)))
            .collect();
        let bucket = Duration::from_millis(100);
        let (full_peak, equal_peak) = (
            largest_bucket(&full, bucket),
            largest_bucket(&equal, bucket),
        );
        assert!(
            equal_peak >= 230,
            "equal jitter fills four buckets: {equal_peak}"
        );
        assert!(full_peak <= 160, "full jitter fills eight: {full_peak}");
        assert!(full.iter().all(|d| *d <= window));
    }

    #[test]
    fn the_first_reconnect_spreads_over_the_first_window() {
        assert_eq!(step_for_attempt(0), 0);
        assert_eq!(step_for_attempt(1), 0);
        assert_eq!(step_for_attempt(2), 1);
        assert_eq!(step_for_attempt(usize::MAX), u32::MAX);
        let backoff = ReconnectConfig::default()
            .backoff()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            backoff.window(step_for_attempt(1)),
            Duration::from_millis(100)
        );
        assert_eq!(
            backoff.window(step_for_attempt(50)),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn a_zero_width_schedule_is_refused() {
        let cases = [
            ReconnectConfig::default().with_initial_delay(Duration::ZERO),
            ReconnectConfig::default().with_backoff_multiplier(0.5),
            ReconnectConfig::default().with_backoff_multiplier(f64::NAN),
            ReconnectConfig::default().with_max_delay(Duration::ZERO),
            ReconnectConfig::default().with_initial_delay(Duration::from_millis(49)),
            ReconnectConfig::default()
                .with_initial_delay(Duration::from_millis(50))
                .with_max_delay(Duration::from_millis(499)),
            ReconnectConfig::default()
                .with_initial_delay(Duration::from_secs(2))
                .with_max_delay(Duration::from_secs(1)),
        ];
        for config in cases {
            match config.backoff() {
                Err(error @ NatsError::Config(_)) => {
                    assert_eq!(error.error_code(), "NATS_015");
                    assert!(error.to_string().contains("reconnect"), "{error}");
                }
                other => panic!("{config:?} was accepted: {other:?}"),
            }
        }
    }

    #[test]
    fn a_removed_setting_fails_to_load() {
        for removed in ["max_attempts = 5", "jitter = false", "enabled = false"] {
            let error = toml::from_str::<ReconnectConfig>(removed)
                .err()
                .unwrap_or_else(|| panic!("`{removed}` loaded"));
            let field = removed.split(' ').next().unwrap_or_default();
            assert!(error.to_string().contains(field), "{error}");
        }
        let kept: ReconnectConfig = toml::from_str("retry_initial_connect = false")
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(kept, ReconnectConfig::fail_fast());
    }

    #[test]
    fn the_floors_themselves_are_accepted() {
        let tightest = ReconnectConfig::default()
            .with_initial_delay(ReconnectConfig::MIN_INITIAL_DELAY)
            .with_max_delay(ReconnectConfig::MIN_MAX_DELAY);
        assert!(tightest.backoff().is_ok());
    }

    #[test]
    fn fail_fast_only_stops_retrying_the_first_connect() {
        let config = ReconnectConfig::fail_fast();
        assert!(!config.retry_initial_connect);
        assert_eq!(
            config.backoff().ok(),
            ReconnectConfig::default().backoff().ok()
        );
    }

    #[test]
    fn builders_set_fields() {
        let cfg = ReconnectConfig::default()
            .with_initial_delay(Duration::from_millis(50))
            .with_max_delay(Duration::from_secs(10))
            .with_backoff_multiplier(1.6);
        assert_eq!(cfg.initial_delay, Duration::from_millis(50));
        assert_eq!(cfg.max_delay, Duration::from_secs(10));
        assert!((cfg.backoff_multiplier - 1.6).abs() < f64::EPSILON);
    }
}
