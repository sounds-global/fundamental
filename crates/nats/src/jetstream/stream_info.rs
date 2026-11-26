use async_nats::jetstream;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::StreamConfig;
use crate::subject::SubjectPrefix;

/// Summary information about a JetStream stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    /// The stream name.
    pub name: String,
    /// Number of messages currently stored.
    pub messages: u64,
    /// Total bytes stored.
    pub bytes: u64,
    /// The first sequence number held.
    pub first_seq: u64,
    /// The last sequence number held.
    pub last_seq: u64,
    /// Number of consumers bound to the stream.
    pub consumer_count: usize,
    /// When the stream was created; a sequence names one message only together with this.
    pub created: DateTime<Utc>,
    /// The configuration the server holds for the stream.
    pub config: StreamConfig,
}

impl StreamInfo {
    pub(crate) fn from_jetstream(
        info: &jetstream::stream::Info,
        prefix: Option<&SubjectPrefix>,
    ) -> Self {
        Self {
            name: info.config.name.clone(),
            messages: info.state.messages,
            bytes: info.state.bytes,
            first_seq: info.state.first_sequence,
            last_seq: info.state.last_sequence,
            consumer_count: info.state.consumer_count,
            created: DateTime::from_timestamp_nanos(
                i64::try_from(info.created.unix_timestamp_nanos()).unwrap_or(i64::MAX),
            ),
            config: StreamConfig::from_jetstream(&info.config, prefix),
        }
    }
}
