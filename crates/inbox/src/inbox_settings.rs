//! [`InboxSettings`] — the inbox as written in configuration.

use fundamental_macros::Validate;
use serde::Deserialize;

/// The `[inbox]` section, before it becomes an [`InboxConfig`](crate::InboxConfig).
#[derive(Clone, Copy, Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct InboxSettings {
    /// How long an applied event is remembered, in hours; an older event arriving is parked.
    #[validate(range(min = 2, max = 8760))]
    pub retention_hours: u32,
    /// How long a parked record is kept for an operator, in hours.
    #[validate(range(min = 24, max = 87600))]
    pub parked_retention_hours: u32,
    /// How many deliveries one listener works on at once.
    #[validate(range(min = 1, max = 256))]
    pub concurrency: u16,
    /// How many deliveries of one event fail before it is parked.
    #[validate(range(min = 1, max = 100))]
    pub max_attempts: u16,
    /// How long one effect may take, in seconds, before it is abandoned and redelivered.
    #[validate(range(min = 1, max = 600))]
    pub effect_deadline_secs: u32,
    /// The longest wait before a retryable failure is delivered again, in seconds.
    #[validate(range(min = 1, max = 3600))]
    pub retry_cap_secs: u32,
    /// How long before a failure that is not retryable is delivered again, in seconds — the
    /// time a deploy has to fix it before the attempts run out.
    #[validate(range(min = 1, max = 86400))]
    pub failed_delay_secs: u32,
    /// How often the sweep runs, in seconds.
    #[validate(range(min = 10, max = 3600))]
    pub sweep_every_secs: u32,
    /// How many rows one sweep statement deletes at most.
    #[validate(range(min = 100, max = 10000))]
    pub sweep_batch: u32,
}

impl Default for InboxSettings {
    fn default() -> Self {
        Self {
            retention_hours: 168,
            parked_retention_hours: 720,
            concurrency: 16,
            max_attempts: 10,
            effect_deadline_secs: 30,
            retry_cap_secs: 60,
            failed_delay_secs: 300,
            sweep_every_secs: 60,
            sweep_batch: 1000,
        }
    }
}
