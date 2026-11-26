use async_nats::jetstream;
use serde::{Deserialize, Serialize};

/// Which messages a full stream discards.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum DiscardPolicy {
    /// Discard the oldest messages.
    #[default]
    Old,
    /// Reject new messages.
    New,
}

impl From<jetstream::stream::DiscardPolicy> for DiscardPolicy {
    fn from(value: jetstream::stream::DiscardPolicy) -> Self {
        match value {
            jetstream::stream::DiscardPolicy::Old => Self::Old,
            jetstream::stream::DiscardPolicy::New => Self::New,
        }
    }
}

impl From<DiscardPolicy> for jetstream::stream::DiscardPolicy {
    fn from(value: DiscardPolicy) -> Self {
        match value {
            DiscardPolicy::Old => Self::Old,
            DiscardPolicy::New => Self::New,
        }
    }
}
