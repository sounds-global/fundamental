use std::time::Duration;

use async_nats::jetstream::{self, AckKind};
use bytes::Bytes;

use super::Delivery;
use crate::error::{NatsError, NatsResult};
use crate::message::{EventId, Message, MessageHeaders};
use crate::subject::{Subject, SubjectPrefix};
use crate::subscription::headers::extract_headers;

/// A JetStream delivery with its payload still bytes, for a consumer that settles each itself;
/// settling consumes it.
#[derive(Debug)]
pub struct JetStreamDelivery {
    /// The payload as stored.
    pub payload: Bytes,
    /// The subject it was published to, without this connection's prefix.
    pub subject: Subject,
    /// Its headers, as published.
    pub headers: MessageHeaders,
    /// Where it sits in its stream and which attempt this is.
    pub delivery: Delivery,
    message: jetstream::Message,
}

impl JetStreamDelivery {
    pub(crate) fn new(
        message: jetstream::Message,
        prefix: Option<&SubjectPrefix>,
    ) -> NatsResult<Self> {
        let delivery = Delivery::of(&message)?;
        let subject = Subject::new_unchecked(prefix.map_or(message.subject.as_str(), |prefix| {
            prefix.strip(message.subject.as_str())
        }));
        Ok(Self {
            payload: Bytes::copy_from_slice(&message.payload),
            subject,
            headers: extract_headers(&message),
            delivery,
            message,
        })
    }

    /// The `Nats-Msg-Id` it was published under, if any.
    #[must_use]
    pub fn msg_id(&self) -> Option<&str> {
        self.message
            .headers
            .as_ref()?
            .get(async_nats::header::NATS_MESSAGE_ID)
            .map(async_nats::HeaderValue::as_str)
    }

    /// Whether it was published as `event` by [`JetStream::publish_event`](crate::JetStream::publish_event):
    /// its `Nats-Msg-Id` is the one [`EventId::msg_id_for`] derives from this payload.
    #[must_use]
    pub fn names_event(&self, event: EventId) -> bool {
        self.msg_id() == Some(event.msg_id_for(&self.payload).as_str())
    }

    /// Decodes the payload as `M`.
    ///
    /// # Errors
    /// Returns [`NatsError::Serialization`] if the payload is not an `M`.
    pub fn decode<M: Message>(&self) -> NatsResult<M> {
        M::from_bytes(&self.payload)
    }

    /// Tells the server the delivery is still being worked on, restarting its ack wait.
    ///
    /// # Errors
    /// Returns [`NatsError::AckFailed`] if the signal cannot be sent.
    pub async fn in_progress(&self) -> NatsResult<()> {
        self.settle_with(AckKind::Progress).await
    }

    /// Acknowledges the delivery, so it is never delivered again.
    ///
    /// # Errors
    /// Returns [`NatsError::AckFailed`] if the acknowledgement cannot be sent.
    pub async fn ack(self) -> NatsResult<()> {
        self.settle_with(AckKind::Ack).await
    }

    /// Asks for the delivery again after `delay`.
    ///
    /// # Errors
    /// Returns [`NatsError::AckFailed`] if the request cannot be sent.
    pub async fn nak(self, delay: Duration) -> NatsResult<()> {
        self.settle_with(AckKind::Nak(Some(delay))).await
    }

    /// Ends the delivery for good: the server will not deliver this message again.
    ///
    /// # Errors
    /// Returns [`NatsError::AckFailed`] if the request cannot be sent.
    pub async fn term(self) -> NatsResult<()> {
        self.settle_with(AckKind::Term).await
    }

    async fn settle_with(&self, kind: AckKind) -> NatsResult<()> {
        self.message
            .ack_with(kind)
            .await
            .map_err(|e| NatsError::AckFailed(e.to_string()))
    }
}
