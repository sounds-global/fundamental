use std::time::Duration;

use fundamental_core::Validate as _;
use fundamental_core::validate::ValidationError;

use crate::{LeaseError, LeaseSettings};

/// Validated lease timings; renewal is derived from the TTL, so it is never slower than the lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaseConfig {
    ttl: Duration,
    renew_every: Duration,
    drain_grace: Duration,
    max_clock_skew: Duration,
    work_retention: Duration,
}

impl LeaseConfig {
    /// The timings `settings` describe.
    ///
    /// # Errors
    ///
    /// [`LeaseError::InvalidConfig`] with every problem.
    pub fn new(settings: LeaseSettings) -> Result<Self, LeaseError> {
        let mut problems = match settings.validate() {
            Ok(()) => fundamental_core::validate::ValidationErrors::new(),
            Err(problems) => problems,
        };
        if u64::from(settings.max_clock_skew_ms) * 4 >= u64::from(settings.ttl_secs) * 1_000 {
            problems.add(ValidationError::new(
                Some("max_clock_skew_ms".to_owned()),
                "lease.skew_too_large",
                "the skew allowance must be under a quarter of the TTL",
            ));
        }
        if !problems.is_empty() {
            return Err(LeaseError::InvalidConfig { problems });
        }
        let ttl = Duration::from_secs(u64::from(settings.ttl_secs));
        Ok(Self {
            ttl,
            renew_every: ttl / settings.renewals_per_ttl.max(1),
            drain_grace: Duration::from_millis(u64::from(settings.drain_grace_ms)),
            max_clock_skew: Duration::from_millis(u64::from(settings.max_clock_skew_ms)),
            work_retention: Duration::from_secs(u64::from(settings.work_retention_hours) * 3_600),
        })
    }

    /// How long a lease lasts without renewal.
    #[must_use]
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// How often the holder renews.
    #[must_use]
    pub fn renew_every(&self) -> Duration {
        self.renew_every
    }

    /// How long a stop waits for in-flight work.
    #[must_use]
    pub fn drain_grace(&self) -> Duration {
        self.drain_grace
    }

    /// How long a committed work key is remembered.
    #[must_use]
    pub fn work_retention(&self) -> Duration {
        self.work_retention
    }

    /// How much of its lease the holder gives up early for clock skew.
    #[must_use]
    pub fn max_clock_skew(&self) -> Duration {
        self.max_clock_skew
    }
}

impl Default for LeaseConfig {
    fn default() -> Self {
        let settings = LeaseSettings::default();
        let ttl = Duration::from_secs(u64::from(settings.ttl_secs));
        Self {
            ttl,
            renew_every: ttl / settings.renewals_per_ttl,
            drain_grace: Duration::from_millis(u64::from(settings.drain_grace_ms)),
            max_clock_skew: Duration::from_millis(u64::from(settings.max_clock_skew_ms)),
            work_retention: Duration::from_secs(u64::from(settings.work_retention_hours) * 3_600),
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn the_config_collects_every_violation() {
        let error = LeaseConfig::new(LeaseSettings {
            ttl_secs: 1,
            renewals_per_ttl: 2,
            drain_grace_ms: 30_000,
            max_clock_skew_ms: 900,
            work_retention_hours: 0,
        })
        .expect_err("five problems");
        assert_eq!(error.error_code(), "SLEASE_004");
        let LeaseError::InvalidConfig { problems } = error else {
            panic!("not a config refusal");
        };
        for field in [
            "ttl_secs",
            "renewals_per_ttl",
            "drain_grace_ms",
            "max_clock_skew_ms",
        ] {
            assert!(
                !problems.errors_for_field(field).is_empty(),
                "{field}: {problems}"
            );
        }
    }

    #[test]
    fn the_renewal_cadence_is_a_fraction_of_the_ttl() {
        let config = LeaseConfig::new(LeaseSettings::default()).expect("the defaults");
        assert_eq!(config, LeaseConfig::default());
        assert_eq!(config.renew_every(), Duration::from_secs(5));
    }
}
