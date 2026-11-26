/// Acknowledgment returned when a message is stored by JetStream.
#[derive(Debug, Clone)]
pub struct PublishAck {
    /// The stream that stored the message.
    pub stream: String,
    /// The sequence number assigned.
    pub sequence: u64,
    /// Whether the server detected this as a duplicate.
    pub duplicate: bool,
}
