use bytes::Bytes;
use uuid::Uuid;

use super::headers::extract_headers;
use crate::error::NatsResult;
use crate::message::{DeadLetter, DeadLetterReason, Message, MessageHeaders};
use crate::subject::Subject;

/// A message received without deserialization.
#[derive(Debug)]
pub struct RawReceivedMessage {
    /// The raw payload bytes.
    pub payload: Bytes,
    /// The subject the message arrived on.
    pub subject: Subject,
    /// The reply subject, if any.
    pub reply: Option<Subject>,
    /// The message headers.
    pub headers: MessageHeaders,
}

impl From<async_nats::Message> for RawReceivedMessage {
    fn from(msg: async_nats::Message) -> Self {
        Self {
            payload: Bytes::copy_from_slice(&msg.payload),
            subject: Subject::new_unchecked(msg.subject.as_str()),
            reply: msg
                .reply
                .as_ref()
                .map(|s| Subject::new_unchecked(s.as_str())),
            headers: extract_headers(&msg),
        }
    }
}

impl RawReceivedMessage {
    /// Deserializes the payload as `T`.
    ///
    /// # Errors
    /// Returns [`NatsError::Serialization`](crate::NatsError::Serialization) if
    /// the payload is not valid `T`.
    pub fn deserialize<T: Message>(&self) -> NatsResult<T> {
        T::from_bytes(&self.payload)
    }

    /// Builds a [`DeadLetter`] for this message with the given `reason`.
    #[must_use]
    pub fn to_dead_letter(&self, reason: DeadLetterReason) -> DeadLetter {
        DeadLetter::new(
            Uuid::new_v4().to_string(),
            self.subject.as_str(),
            String::from_utf8_lossy(&self.payload),
            reason,
        )
    }
}
