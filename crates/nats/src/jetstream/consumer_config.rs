use std::time::Duration;

use async_nats::jetstream::consumer::pull::Config as PullConfig;
use serde::{Deserialize, Serialize};

use super::{AckPolicy, DeliverPolicy, ReplayPolicy};

/// Configuration for creating or updating a JetStream pull consumer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumerConfig {
    /// The consumer name.
    pub name: String,
    /// The durable name; set to make the consumer survive restarts.
    #[serde(default)]
    pub durable_name: Option<String>,
    /// A human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// Where delivery starts.
    #[serde(default)]
    pub deliver_policy: DeliverPolicy,
    /// Acknowledgment policy.
    #[serde(default)]
    pub ack_policy: AckPolicy,
    /// How long to wait for an ack before redelivery.
    #[serde(default)]
    pub ack_wait: Option<Duration>,
    /// Maximum delivery attempts.
    #[serde(default)]
    pub max_deliver: Option<i64>,
    /// A subject filter.
    #[serde(default)]
    pub filter_subject: Option<String>,
    /// Several subject filters; used instead of `filter_subject` when not empty.
    #[serde(default)]
    pub filter_subjects: Vec<String>,
    /// The delay before each successive redelivery; the last step repeats.
    #[serde(default)]
    pub backoff: Vec<Duration>,
    /// Replay rate.
    #[serde(default)]
    pub replay_policy: ReplayPolicy,
    /// Maximum un-acked messages allowed in flight.
    #[serde(default)]
    pub max_ack_pending: Option<i64>,
}

impl ConsumerConfig {
    /// Creates a consumer config named `name` with explicit-ack defaults.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            durable_name: None,
            description: None,
            deliver_policy: DeliverPolicy::All,
            ack_policy: AckPolicy::Explicit,
            ack_wait: None,
            max_deliver: None,
            filter_subject: None,
            filter_subjects: Vec::new(),
            backoff: Vec::new(),
            replay_policy: ReplayPolicy::Instant,
            max_ack_pending: None,
        }
    }

    /// Makes the consumer durable, using its name as the durable name.
    #[must_use]
    pub fn durable(mut self) -> Self {
        self.durable_name = Some(self.name.clone());
        self
    }

    /// Sets the deliver policy.
    #[must_use]
    pub fn deliver_policy(mut self, policy: DeliverPolicy) -> Self {
        self.deliver_policy = policy;
        self
    }

    /// Sets the ack policy.
    #[must_use]
    pub fn ack_policy(mut self, policy: AckPolicy) -> Self {
        self.ack_policy = policy;
        self
    }

    /// Sets the ack-wait duration.
    #[must_use]
    pub fn ack_wait(mut self, wait: Duration) -> Self {
        self.ack_wait = Some(wait);
        self
    }

    /// Sets the maximum delivery attempts.
    #[must_use]
    pub fn max_deliver(mut self, max: i64) -> Self {
        self.max_deliver = Some(max);
        self
    }

    /// Sets the subject filter.
    #[must_use]
    pub fn filter_subject(mut self, subject: impl Into<String>) -> Self {
        self.filter_subject = Some(subject.into());
        self
    }

    /// Sets several subject filters at once.
    #[must_use]
    pub fn filter_subjects<I, S>(mut self, subjects: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.filter_subjects = subjects.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the delay before each successive redelivery.
    #[must_use]
    pub fn backoff<I>(mut self, steps: I) -> Self
    where
        I: IntoIterator<Item = Duration>,
    {
        self.backoff = steps.into_iter().collect();
        self
    }

    /// Sets the maximum number of delivered, unacknowledged messages in flight.
    #[must_use]
    pub fn max_ack_pending(mut self, max: i64) -> Self {
        self.max_ack_pending = Some(max);
        self
    }

    pub(crate) fn into_jetstream_config(self) -> PullConfig {
        let mut config = PullConfig {
            name: Some(self.name),
            durable_name: self.durable_name,
            description: self.description,
            deliver_policy: self.deliver_policy.into(),
            ack_policy: self.ack_policy.into(),
            replay_policy: self.replay_policy.into(),
            filter_subject: self.filter_subject.unwrap_or_default(),
            filter_subjects: self.filter_subjects,
            backoff: self.backoff,
            ..Default::default()
        };
        if let Some(wait) = self.ack_wait {
            config.ack_wait = wait;
        }
        if let Some(max) = self.max_deliver {
            config.max_deliver = max;
        }
        if let Some(max) = self.max_ack_pending {
            config.max_ack_pending = max;
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_fields() {
        let cfg = ConsumerConfig::new("processor")
            .durable()
            .ack_policy(AckPolicy::Explicit)
            .max_deliver(5);
        assert_eq!(cfg.name, "processor");
        assert_eq!(cfg.durable_name.as_deref(), Some("processor"));
        assert_eq!(cfg.max_deliver, Some(5));
    }
}
