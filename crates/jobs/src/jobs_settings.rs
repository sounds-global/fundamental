use std::time::Duration;

use chrono::TimeDelta;
use fundamental_core::Validate as _;
use fundamental_core::validate::ValidationError;
use fundamental_db::RetryPolicy;
use fundamental_macros::Validate;
use serde::Deserialize;

use crate::JobError;

/// How a [`JobsSurface`](crate::JobsSurface) drains, retries and purges, as written in
/// configuration under `[jobs]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct JobsSettings {
    /// How often each replica claims due jobs, in milliseconds — the most a job waits past its
    /// due time on an idle table.
    #[validate(range(min = 100, max = 60000))]
    pub poll_interval_ms: u32,
    /// How many due jobs one claim takes from one bucket.
    #[validate(range(min = 1, max = 1000))]
    pub batch_per_bucket: u32,
    /// How many buckets one replica drains at once.
    #[validate(range(min = 1, max = 64))]
    pub concurrency: u32,
    /// How long a claimed job stays invisible to other replicas, in seconds: longer than one
    /// batch's handlers take, or a slow batch's later jobs are claimed again and superseded.
    #[validate(range(min = 1, max = 3600))]
    pub lease_secs: u32,
    /// The delay after a job's first failure, in seconds, doubled on each further one.
    #[validate(range(min = 1, max = 3600))]
    pub backoff_base_secs: u32,
    /// The longest delay between two attempts, in seconds.
    #[validate(range(min = 1, max = 86400))]
    pub backoff_max_secs: u32,
    /// How long a fired job's record is kept, in hours; at least
    /// [`MIN_RETENTION_HOURS`](Self::MIN_RETENTION_HOURS).
    #[validate(range(min = 168, max = 87600))]
    pub retention_hours: u32,
    /// How often fired records past retention are purged, in seconds.
    #[validate(range(min = 1, max = 3600))]
    pub purge_interval_secs: u32,
    /// How many fired records one purge removes from one bucket.
    #[validate(range(min = 1, max = 5000))]
    pub purge_batch: u32,
    /// How often the job table is inventoried, in seconds.
    #[validate(range(min = 5, max = 3600))]
    pub inventory_interval_secs: u32,
    /// How long a job that answered [`Fire::Defer`](crate::Fire::Defer) waits before it is asked
    /// again, in seconds.
    #[validate(range(min = 1, max = 3600))]
    pub defer_secs: u32,
    /// How long past its due time a pending job is before the inventory warns of it, in seconds.
    #[validate(range(min = 60, max = 604_800))]
    pub overdue_warn_secs: u32,
}

impl JobsSettings {
    /// The shortest a fired job's record is kept, in hours; scheduling refuses a due time older
    /// than this, so a purged occurrence can never be scheduled and fired again.
    pub const MIN_RETENTION_HOURS: u32 = 168;

    /// [`MIN_RETENTION_HOURS`](Self::MIN_RETENTION_HOURS) as a span.
    #[must_use]
    pub fn min_retention() -> TimeDelta {
        TimeDelta::hours(i64::from(Self::MIN_RETENTION_HOURS))
    }

    /// These settings, checked.
    ///
    /// # Errors
    ///
    /// [`JobError::InvalidSettings`] with every problem: a value out of range, or a backoff whose
    /// first delay exceeds its longest.
    pub fn checked(self) -> Result<Self, JobError> {
        let mut problems = self.validate().err().unwrap_or_default();
        if self.backoff_base_secs > self.backoff_max_secs {
            problems.add(ValidationError::new(
                Some("backoff_base_secs".to_owned()),
                "jobs.backoff_inverted",
                "the first backoff may not exceed the longest",
            ));
        }
        if problems.is_empty() {
            Ok(self)
        } else {
            Err(JobError::InvalidSettings { problems })
        }
    }

    /// The lease and backoff the drain claims and retries under.
    #[must_use]
    pub fn retry_policy(&self) -> RetryPolicy {
        RetryPolicy::new(
            Duration::from_secs(u64::from(self.lease_secs)),
            Duration::from_secs(u64::from(self.backoff_base_secs)),
            Duration::from_secs(u64::from(self.backoff_max_secs)),
        )
    }

    /// How long a deferred job waits before it is asked again: never less than a second, whatever
    /// the field says, so no handler can make a job hot-loop.
    #[must_use]
    pub fn defer(&self) -> Duration {
        Duration::from_secs(u64::from(self.defer_secs.max(1)))
    }

    /// How long past its due time a pending job is before the inventory warns of it.
    #[must_use]
    pub fn overdue_warn(&self) -> TimeDelta {
        TimeDelta::seconds(i64::from(self.overdue_warn_secs))
    }

    /// How long a fired record is kept: never less than
    /// [`MIN_RETENTION_HOURS`](Self::MIN_RETENTION_HOURS), whatever the field says.
    #[must_use]
    pub fn retention(&self) -> TimeDelta {
        TimeDelta::hours(i64::from(
            self.retention_hours.max(Self::MIN_RETENTION_HOURS),
        ))
    }

    /// How often each replica claims.
    #[must_use]
    pub fn poll_interval(&self) -> Duration {
        Duration::from_millis(u64::from(self.poll_interval_ms))
    }

    /// How often fired records are purged.
    #[must_use]
    pub fn purge_interval(&self) -> Duration {
        Duration::from_secs(u64::from(self.purge_interval_secs))
    }

    /// How often the table is inventoried.
    #[must_use]
    pub fn inventory_interval(&self) -> Duration {
        Duration::from_secs(u64::from(self.inventory_interval_secs))
    }
}

impl Default for JobsSettings {
    fn default() -> Self {
        Self {
            poll_interval_ms: 1_000,
            batch_per_bucket: 100,
            concurrency: 4,
            lease_secs: 30,
            backoff_base_secs: 5,
            backoff_max_secs: 900,
            retention_hours: Self::MIN_RETENTION_HOURS,
            purge_interval_secs: 60,
            purge_batch: 500,
            inventory_interval_secs: 60,
            defer_secs: 30,
            overdue_warn_secs: 900,
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn the_defaults_are_valid() {
        assert_eq!(
            JobsSettings::default().checked().expect("the defaults"),
            JobsSettings::default()
        );
    }

    #[test]
    fn every_problem_with_the_settings_is_collected() {
        let error = JobsSettings {
            poll_interval_ms: 10,
            batch_per_bucket: 0,
            retention_hours: 24,
            backoff_base_secs: 600,
            backoff_max_secs: 60,
            defer_secs: 0,
            overdue_warn_secs: 59,
            ..JobsSettings::default()
        }
        .checked()
        .expect_err("refused");
        assert_eq!(error.error_code(), "JOBS_010");
        let JobError::InvalidSettings { problems } = error else {
            panic!("not a settings refusal: {error:?}");
        };
        for field in [
            "poll_interval_ms",
            "batch_per_bucket",
            "retention_hours",
            "backoff_base_secs",
            "defer_secs",
            "overdue_warn_secs",
        ] {
            assert!(
                !problems.errors_for_field(field).is_empty(),
                "{field}: {problems}"
            );
        }
    }

    #[test]
    fn unchecked_settings_still_keep_a_fired_record_a_week() {
        let short = JobsSettings {
            retention_hours: 24,
            ..JobsSettings::default()
        };
        assert_eq!(short.retention(), JobsSettings::min_retention());
    }

    #[test]
    fn unchecked_settings_still_defer_at_least_a_second() {
        let zero = JobsSettings {
            defer_secs: 0,
            ..JobsSettings::default()
        };
        assert_eq!(zero.defer(), Duration::from_secs(1));
    }

    #[test]
    fn a_defer_or_overdue_threshold_past_its_bound_is_refused() {
        for settings in [
            JobsSettings {
                defer_secs: 3_601,
                ..JobsSettings::default()
            },
            JobsSettings {
                overdue_warn_secs: 604_801,
                ..JobsSettings::default()
            },
        ] {
            let error = settings.checked().expect_err("refused");
            assert_eq!(error.error_code(), "JOBS_010");
        }
    }

    #[test]
    fn retention_can_never_be_shorter_than_the_schedule_floor() {
        let error = JobsSettings {
            retention_hours: JobsSettings::MIN_RETENTION_HOURS - 1,
            ..JobsSettings::default()
        }
        .checked()
        .expect_err("refused");
        assert_eq!(error.error_code(), "JOBS_010");
    }
}
