use bytes::Bytes;

use super::{Message, MessageHeaders};
use crate::error::NatsResult;
use crate::subject::Subject;

#[cfg(feature = "jetstream")]
use crate::error::NatsError;
#[cfg(feature = "jetstream")]
use std::time::Duration;

/// A message received from a subscription, with its payload deserialized to `T`.
#[derive(Debug)]
pub struct ReceivedMessage<T> {
    /// The deserialized payload.
    pub payload: T,
    /// The subject the message arrived on.
    pub subject: Subject,
    /// The reply subject, for request-reply.
    pub reply: Option<Subject>,
    /// The message headers.
    pub headers: MessageHeaders,
    raw: Bytes,
    #[cfg(feature = "jetstream")]
    jetstream_message: Option<async_nats::jetstream::Message>,
}

impl<T> ReceivedMessage<T> {
    pub(crate) fn new(
        payload: T,
        subject: Subject,
        reply: Option<Subject>,
        headers: MessageHeaders,
        raw: Bytes,
    ) -> Self {
        Self {
            payload,
            subject,
            reply,
            headers,
            raw,
            #[cfg(feature = "jetstream")]
            jetstream_message: None,
        }
    }

    /// `payload` as a subscriber receives it, with no reply and no headers, for calling a
    /// `#[subscribe]` handler in a test.
    ///
    /// ```
    /// # use fundamental_nats::{Message, ReceivedMessage};
    /// # use serde::{Deserialize, Serialize};
    /// #[derive(Serialize, Deserialize, Message)]
    /// #[subject = "events.hello"]
    /// struct Hello {
    ///     name: String,
    /// }
    ///
    /// let message = ReceivedMessage::delivered(Hello { name: "Ada".to_owned() })?;
    /// assert_eq!(message.subject.as_str(), "events.hello");
    /// # Ok::<(), fundamental_nats::NatsError>(())
    /// ```
    ///
    /// # Errors
    /// [`NatsError::Serialization`](crate::NatsError::Serialization) when `payload` does not
    /// serialize, and the subject's own error when it names no valid subject.
    pub fn delivered(payload: T) -> NatsResult<Self>
    where
        T: Message,
    {
        let raw = payload.to_bytes()?;
        let subject = Subject::new(payload.subject())?;
        Ok(Self::new(
            payload,
            subject,
            None,
            MessageHeaders::default(),
            raw,
        ))
    }

    /// Returns the raw message bytes.
    #[must_use]
    pub fn raw_bytes(&self) -> &Bytes {
        &self.raw
    }

    /// Returns whether the sender expects a reply.
    #[must_use]
    pub fn expects_reply(&self) -> bool {
        self.reply.is_some()
    }

    /// Maps the payload, preserving subject, reply, headers, and raw bytes.
    #[must_use]
    pub fn map<U, F>(self, f: F) -> ReceivedMessage<U>
    where
        F: FnOnce(T) -> U,
    {
        ReceivedMessage {
            payload: f(self.payload),
            subject: self.subject,
            reply: self.reply,
            headers: self.headers,
            raw: self.raw,
            #[cfg(feature = "jetstream")]
            jetstream_message: self.jetstream_message,
        }
    }
}

#[cfg(feature = "jetstream")]
impl<T> ReceivedMessage<T> {
    pub(crate) fn with_jetstream_message(mut self, msg: async_nats::jetstream::Message) -> Self {
        self.jetstream_message = Some(msg);
        self
    }

    /// Removes and returns the JetStream ack handle, so the dispatch loop acks for `auto_ack`.
    pub(crate) fn take_jetstream_message(&mut self) -> Option<async_nats::jetstream::Message> {
        self.jetstream_message.take()
    }

    /// Acknowledges the message so JetStream stops redelivering it.
    ///
    /// # Errors
    /// Returns [`NatsError::AckFailed`] if the acknowledgment cannot be sent.
    pub async fn ack(&self) -> NatsResult<()> {
        self.ack_with(async_nats::jetstream::AckKind::Ack).await
    }

    /// Negatively acknowledges the message, requesting immediate redelivery.
    ///
    /// # Errors
    /// Returns [`NatsError::AckFailed`] if the acknowledgment cannot be sent.
    pub async fn nack(&self) -> NatsResult<()> {
        self.ack_with(async_nats::jetstream::AckKind::Nak(None))
            .await
    }

    /// Negatively acknowledges with a delay before redelivery.
    ///
    /// # Errors
    /// Returns [`NatsError::AckFailed`] if the acknowledgment cannot be sent.
    pub async fn nack_with_delay(&self, delay: Duration) -> NatsResult<()> {
        self.ack_with(async_nats::jetstream::AckKind::Nak(Some(delay)))
            .await
    }

    /// Terminates the message so it is not redelivered.
    ///
    /// # Errors
    /// Returns [`NatsError::AckFailed`] if the acknowledgment cannot be sent.
    pub async fn term(&self) -> NatsResult<()> {
        self.ack_with(async_nats::jetstream::AckKind::Term).await
    }

    /// Signals that processing is still in progress, resetting the ack timeout.
    ///
    /// # Errors
    /// Returns [`NatsError::AckFailed`] if the acknowledgment cannot be sent.
    pub async fn in_progress(&self) -> NatsResult<()> {
        self.ack_with(async_nats::jetstream::AckKind::Progress)
            .await
    }

    async fn ack_with(&self, kind: async_nats::jetstream::AckKind) -> NatsResult<()> {
        if let Some(msg) = &self.jetstream_message {
            msg.ack_with(kind)
                .await
                .map_err(|e| NatsError::AckFailed(e.to_string()))?;
        }
        Ok(())
    }
}
