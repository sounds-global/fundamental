use fundamental_macros::Validate;
use serde::Deserialize;

/// Lease settings as written in configuration.
#[derive(Clone, Copy, Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct LeaseSettings {
    /// How long a lease lasts without renewal, in seconds.
    #[validate(range(min = 2, max = 600))]
    pub ttl_secs: u32,
    /// How many renewals fit in one TTL.
    #[validate(range(min = 3, max = 20))]
    pub renewals_per_ttl: u32,
    /// How long in-flight work may finish when the surface stops, in milliseconds.
    #[validate(range(min = 0, max = 20000))]
    pub drain_grace_ms: u32,
    /// The most two database nodes' clocks may disagree, in milliseconds.
    #[validate(range(min = 0, max = 5000))]
    pub max_clock_skew_ms: u32,
    /// How long a committed work key is remembered, in hours; at least the longest retry of one piece of work.
    #[validate(range(min = 1, max = 87600))]
    pub work_retention_hours: u32,
}

impl Default for LeaseSettings {
    fn default() -> Self {
        Self {
            ttl_secs: 15,
            renewals_per_ttl: 3,
            drain_grace_ms: 5_000,
            max_clock_skew_ms: 500,
            work_retention_hours: 720,
        }
    }
}
