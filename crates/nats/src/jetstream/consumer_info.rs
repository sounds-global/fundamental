use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::AckPolicy;

/// Summary information about a JetStream consumer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumerInfo {
    /// The consumer name.
    pub name: String,
    /// The stream the consumer is bound to.
    pub stream: String,
    /// Messages not yet delivered.
    pub num_pending: u64,
    /// Outstanding pull requests.
    pub num_waiting: usize,
    /// Messages delivered but not yet acknowledged.
    pub num_ack_pending: usize,
    /// Messages that have been redelivered.
    pub num_redelivered: usize,
    /// How deliveries must be acknowledged.
    pub ack_policy: AckPolicy,
    /// How many times a message is delivered before the server gives up on it; `-1` is
    /// without limit.
    pub max_deliver: i64,
    /// How long the server waits for an acknowledgement before delivering again.
    pub ack_wait: Duration,
}
