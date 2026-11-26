//! [`InboxConfig`] — validated inbox settings, as durations.

use std::time::Duration;

use chrono::TimeDelta;
use fundamental_core::Validate as _;
use fundamental_core::retry::{Backoff, Entropy};
use fundamental_core::validate::{ValidationError, ValidationErrors};

use crate::{InboxError, InboxSettings};

/// Validated inbox settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InboxConfig {
    retention: Duration,
    parked_retention: Duration,
    concurrency: u16,
    max_attempts: u16,
    effect_deadline: Duration,
    retry: Backoff,
    failed_delay: Duration,
    sweep_every: Duration,
    sweep_batch: u32,
}

const MARGIN: Duration = Duration::from_secs(3_600);
const MAX_SKEW: Duration = Duration::from_secs(300);

/// The first retry's window; each later one doubles, up to `retry_cap_secs`.
const FIRST_RETRY: Duration = Duration::from_secs(1);

impl InboxConfig {
    /// The durations `settings` describe.
    ///
    /// # Errors
    ///
    /// [`InboxError::InvalidSettings`] with every value out of range.
    pub fn new(settings: InboxSettings) -> Result<Self, InboxError> {
        settings
            .validate()
            .map_err(|problems| InboxError::InvalidSettings { problems })?;
        let secs = |value: u32| Duration::from_secs(u64::from(value));
        let retry =
            Backoff::new(FIRST_RETRY, 2.0, secs(settings.retry_cap_secs)).map_err(|rejection| {
                let mut problems = ValidationErrors::new();
                problems.add(ValidationError::new(
                    Some("retry_cap_secs".to_owned()),
                    "inbox.retry_schedule",
                    rejection.to_string(),
                ));
                InboxError::InvalidSettings { problems }
            })?;
        let hours = |value: u32| Duration::from_secs(u64::from(value) * 3_600);
        Ok(Self {
            retention: hours(settings.retention_hours),
            parked_retention: hours(settings.parked_retention_hours),
            concurrency: settings.concurrency,
            max_attempts: settings.max_attempts,
            effect_deadline: secs(settings.effect_deadline_secs),
            retry,
            failed_delay: secs(settings.failed_delay_secs),
            sweep_every: secs(settings.sweep_every_secs),
            sweep_batch: settings.sweep_batch,
        })
    }

    /// How long an applied event's row is kept.
    #[must_use]
    pub fn retention(&self) -> Duration {
        self.retention
    }

    /// How old an event may be and still be applied: the retention less an hour.
    #[must_use]
    pub fn acceptance_window(&self) -> TimeDelta {
        delta(self.retention.saturating_sub(MARGIN))
    }

    /// How far ahead of the database's clock an event's timestamp may be.
    #[must_use]
    pub fn max_skew(&self) -> TimeDelta {
        delta(MAX_SKEW)
    }

    /// How long a parked record is kept.
    #[must_use]
    pub fn parked_retention(&self) -> Duration {
        self.parked_retention
    }

    /// How many deliveries one listener works on at once.
    #[must_use]
    pub fn concurrency(&self) -> usize {
        usize::from(self.concurrency)
    }

    /// How many failed deliveries park an event.
    #[must_use]
    pub fn max_attempts(&self) -> u64 {
        u64::from(self.max_attempts)
    }

    /// How long one effect may take.
    #[must_use]
    pub fn effect_deadline(&self) -> Duration {
        self.effect_deadline
    }

    /// The wait before redelivering after a retryable failure on attempt `delivered`: a full-jitter
    /// draw from a window starting at a second and doubling to the retry cap.
    #[must_use]
    pub fn retry_delay(&self, delivered: u64, entropy: &mut impl Entropy) -> Duration {
        self.retry.draw(step(delivered), None, entropy)
    }

    /// The widest wait [`retry_delay`](Self::retry_delay) can draw on attempt `delivered`.
    #[must_use]
    pub fn retry_window(&self, delivered: u64) -> Duration {
        self.retry.window(step(delivered))
    }

    /// The wait before redelivering after a failure that is not retryable.
    #[must_use]
    pub fn failed_delay(&self) -> Duration {
        self.failed_delay
    }

    /// How often the sweep runs.
    #[must_use]
    pub fn sweep_every(&self) -> Duration {
        self.sweep_every
    }

    /// How many rows one sweep statement deletes at most.
    #[must_use]
    pub fn sweep_batch(&self) -> u32 {
        self.sweep_batch
    }
}

impl Default for InboxConfig {
    fn default() -> Self {
        let settings = InboxSettings::default();
        Self {
            retention: Duration::from_secs(u64::from(settings.retention_hours) * 3_600),
            parked_retention: Duration::from_secs(
                u64::from(settings.parked_retention_hours) * 3_600,
            ),
            concurrency: settings.concurrency,
            max_attempts: settings.max_attempts,
            effect_deadline: Duration::from_secs(u64::from(settings.effect_deadline_secs)),
            retry: DEFAULT_RETRY,
            failed_delay: Duration::from_secs(u64::from(settings.failed_delay_secs)),
            sweep_every: Duration::from_secs(u64::from(settings.sweep_every_secs)),
            sweep_batch: settings.sweep_batch,
        }
    }
}

/// The default schedule, checked when the crate compiles rather than when it runs.
const DEFAULT_RETRY: Backoff = match Backoff::new(FIRST_RETRY, 2.0, Duration::from_secs(60)) {
    Ok(backoff) => backoff,
    Err(_) => panic!("the default retry schedule is valid"),
};

/// The attempt number as a backoff step: the first attempt's retry is step zero.
fn step(delivered: u64) -> u32 {
    u32::try_from(delivered.saturating_sub(1)).unwrap_or(u32::MAX)
}

fn delta(duration: Duration) -> TimeDelta {
    TimeDelta::from_std(duration).unwrap_or(TimeDelta::MAX)
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn the_defaults_are_a_valid_config() {
        assert_eq!(
            InboxConfig::new(InboxSettings::default()).expect("valid"),
            InboxConfig::default()
        );
    }

    #[test]
    fn every_out_of_range_value_is_reported_together() {
        let error = InboxConfig::new(InboxSettings {
            retention_hours: 1,
            concurrency: 0,
            sweep_batch: 5,
            ..InboxSettings::default()
        })
        .expect_err("out of range");
        assert_eq!(error.error_code(), "SINBOX_003");
        let InboxError::InvalidSettings { problems } = error else {
            panic!("not InvalidSettings: {error:?}");
        };
        for field in ["retention_hours", "concurrency", "sweep_batch"] {
            assert!(
                !problems.errors_for_field(field).is_empty(),
                "{field} not reported"
            );
        }
    }

    #[test]
    fn the_acceptance_window_is_an_hour_inside_the_retention() {
        let config = InboxConfig::default();
        assert_eq!(config.acceptance_window(), TimeDelta::hours(167));
    }

    #[test]
    fn a_retry_window_doubles_from_a_second_to_its_cap() {
        let config = InboxConfig::default();
        assert_eq!(config.retry_window(1), Duration::from_secs(1));
        assert_eq!(config.retry_window(2), Duration::from_secs(2));
        assert_eq!(config.retry_window(6), Duration::from_secs(32));
        assert_eq!(config.retry_window(7), Duration::from_secs(60));
        assert_eq!(config.retry_window(u64::MAX), Duration::from_secs(60));
    }
}
