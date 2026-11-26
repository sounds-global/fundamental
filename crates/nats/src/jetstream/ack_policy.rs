use async_nats::jetstream;
use serde::{Deserialize, Serialize};

/// Acknowledgment requirement for a consumer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum AckPolicy {
    /// Each message must be acknowledged individually.
    #[default]
    Explicit,
    /// No acknowledgment is required.
    None,
    /// Acknowledging a message acknowledges all earlier ones.
    All,
}

impl From<AckPolicy> for jetstream::consumer::AckPolicy {
    fn from(value: AckPolicy) -> Self {
        match value {
            AckPolicy::Explicit => Self::Explicit,
            AckPolicy::None => Self::None,
            AckPolicy::All => Self::All,
        }
    }
}

impl From<jetstream::consumer::AckPolicy> for AckPolicy {
    fn from(value: jetstream::consumer::AckPolicy) -> Self {
        match value {
            jetstream::consumer::AckPolicy::Explicit => Self::Explicit,
            jetstream::consumer::AckPolicy::None => Self::None,
            // A server-managed sourcing consumer's policy, never an application's.
            _ => Self::All,
        }
    }
}
