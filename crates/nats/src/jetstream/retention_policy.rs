use async_nats::jetstream;
use serde::{Deserialize, Serialize};

/// How long a stream retains messages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum RetentionPolicy {
    /// Keep messages until a configured limit is reached.
    #[default]
    Limits,
    /// Remove a message once every consumer has acknowledged it.
    Interest,
    /// Remove a message as soon as it is acknowledged (work queue).
    WorkQueue,
}

impl From<jetstream::stream::RetentionPolicy> for RetentionPolicy {
    fn from(value: jetstream::stream::RetentionPolicy) -> Self {
        match value {
            jetstream::stream::RetentionPolicy::Limits => Self::Limits,
            jetstream::stream::RetentionPolicy::Interest => Self::Interest,
            jetstream::stream::RetentionPolicy::WorkQueue => Self::WorkQueue,
        }
    }
}

impl From<RetentionPolicy> for jetstream::stream::RetentionPolicy {
    fn from(value: RetentionPolicy) -> Self {
        match value {
            RetentionPolicy::Limits => Self::Limits,
            RetentionPolicy::Interest => Self::Interest,
            RetentionPolicy::WorkQueue => Self::WorkQueue,
        }
    }
}
