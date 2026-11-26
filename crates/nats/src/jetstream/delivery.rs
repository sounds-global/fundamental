use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{NatsError, NatsResult};

/// What the server says about one delivery of a stored message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    /// The stream the message is stored in.
    pub stream: String,
    /// Its sequence in that stream; the same on every delivery of this stored copy.
    pub stream_sequence: u64,
    /// Its sequence in this consumer's deliveries.
    pub consumer_sequence: u64,
    /// Which attempt this is, counting from 1.
    pub delivered: u64,
    /// When the server stored it.
    pub published_at: DateTime<Utc>,
}

impl Delivery {
    /// Reads the delivery metadata a JetStream message carries in its reply subject.
    pub(crate) fn of(message: &async_nats::jetstream::Message) -> NatsResult<Self> {
        let info = message
            .info()
            .map_err(|e| NatsError::JetStream(format!("delivery metadata: {e}")))?;
        let published_at = DateTime::from_timestamp_nanos(
            i64::try_from(info.published.unix_timestamp_nanos()).unwrap_or(i64::MAX),
        );
        Ok(Self {
            stream: info.stream.to_owned(),
            stream_sequence: info.stream_sequence,
            consumer_sequence: info.consumer_sequence,
            delivered: u64::try_from(info.delivered).unwrap_or(0),
            published_at,
        })
    }
}
