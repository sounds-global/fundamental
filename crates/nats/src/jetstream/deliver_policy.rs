use async_nats::jetstream;
use serde::{Deserialize, Serialize};

/// The point in a stream a consumer begins delivering from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum DeliverPolicy {
    /// Deliver every message.
    #[default]
    All,
    /// Deliver from the last message onward.
    Last,
    /// Deliver only messages arriving after the consumer is created.
    New,
    /// Deliver the last message for each subject.
    LastPerSubject,
}

impl From<DeliverPolicy> for jetstream::consumer::DeliverPolicy {
    fn from(value: DeliverPolicy) -> Self {
        match value {
            DeliverPolicy::All => Self::All,
            DeliverPolicy::Last => Self::Last,
            DeliverPolicy::New => Self::New,
            DeliverPolicy::LastPerSubject => Self::LastPerSubject,
        }
    }
}
