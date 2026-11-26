use async_nats::jetstream;
use serde::{Deserialize, Serialize};

/// How fast a consumer replays stored messages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ReplayPolicy {
    /// Replay as fast as possible.
    #[default]
    Instant,
    /// Replay at the original message timing.
    Original,
}

impl From<ReplayPolicy> for jetstream::consumer::ReplayPolicy {
    fn from(value: ReplayPolicy) -> Self {
        match value {
            ReplayPolicy::Instant => Self::Instant,
            ReplayPolicy::Original => Self::Original,
        }
    }
}
