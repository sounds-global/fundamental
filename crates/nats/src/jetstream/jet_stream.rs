use std::future::Future;

use async_nats::jetstream;
use bytes::Bytes;
use fundamental_core::Error as _;
use futures::StreamExt;
use tracing::{Instrument, debug, error, info, info_span, instrument, warn};

use async_nats::jetstream::ErrorCode;
use async_nats::jetstream::context::{CreateStreamErrorKind, GetStreamErrorKind};

use super::stream_drift::stream_drift;
use super::{JetStreamStream, PublishAck, StreamConfig, StreamInfo};
use crate::client::Wire;
use crate::error::{NatsError, NatsResult};
use crate::message::{Event, Message, MessageHeaders, ReceivedMessage};
use crate::subscription::Listener;
use crate::tracing::DeliveryIds;

/// Headers carrying the current span's trace context, and nothing else.
///
/// So a consumer, however much later, can trace why the message exists.
fn traced_headers() -> MessageHeaders {
    let mut headers = MessageHeaders::new();
    fundamental_observability::trace_context::inject_current(&mut headers);
    headers
}

/// A JetStream context: create and inspect streams and consumers, and publish
/// messages that are persisted and acknowledged.
#[derive(Debug, Clone)]
pub struct JetStream {
    inner: jetstream::Context,
    /// The connection this context was taken from, for the subjects it has to name.
    ///
    /// A stream's captured subjects and every publish both resolve through the same [`Wire`](crate::client),
    /// so a prefixed connection's stream and publishes cannot disagree.
    wire: Wire,
}

impl JetStream {
    pub(crate) fn new(wire: Wire) -> Self {
        Self {
            inner: jetstream::new(wire.raw().clone()),
            wire,
        }
    }

    /// Creates a stream, or returns the one already there when it is exactly what `config`
    /// declares.
    ///
    ///
    /// Never updates, since an update can shrink retention into a purge; a stream that differs
    /// is refused, naming every field. Concurrent creators both end with the stream.
    ///
    /// # Errors
    /// [`NatsError::StreamDrift`] when the stream exists and differs;
    /// [`NatsError::StreamLookupFailed`] when whether it exists could not be learnt;
    /// [`NatsError::JetStream`] when the server refuses the creation.
    #[instrument(skip(self, config), fields(stream = %config.name), err(Debug))]
    pub async fn create_stream(&self, mut config: StreamConfig) -> NatsResult<JetStreamStream> {
        let name = config.name.clone();
        // The captured subjects move with the prefix, or a prefixed publish is stored nowhere.
        config.subjects = config
            .subjects
            .iter()
            .map(|subject| self.wire.resolve(subject))
            .collect();
        let declared = config.into_jetstream_config();

        match self.get_stream(&name).await {
            Ok(existing) => return self.verified(existing, &declared),
            Err(NatsError::StreamNotFound(_)) => {}
            Err(other) => return Err(other),
        }
        match self.inner.create_stream(declared.clone()).await {
            Ok(stream) => {
                info!(stream = %name, "created stream");
                Ok(JetStreamStream::new(stream, self.wire.prefix().cloned()))
            }
            Err(error)
                if matches!(error.kind(), CreateStreamErrorKind::JetStream(e)
                    if e.error_code() == ErrorCode::STREAM_NAME_EXIST) =>
            {
                debug!(stream = %name, "another caller created the stream first; verifying it");
                let existing = self.get_stream(&name).await?;
                self.verified(existing, &declared)
            }
            Err(error) => Err(NatsError::JetStream(format!("creating {name}: {error}"))),
        }
    }

    fn verified(
        &self,
        existing: JetStreamStream,
        declared: &jetstream::stream::Config,
    ) -> NatsResult<JetStreamStream> {
        let drift = stream_drift(declared, &existing.cached_config());
        if drift.is_empty() {
            info!(stream = %declared.name, "stream exists as declared");
            return Ok(existing);
        }
        let fields = drift.join(", ");
        let refusal = NatsError::StreamDrift {
            stream: declared.name.clone(),
            fields,
        };
        error!(
            stream = %declared.name,
            code = refusal.error_code(),
            error = %refusal,
            "a stream exists under the declared name with another configuration; not updating it"
        );
        Err(refusal)
    }

    /// Looks up an existing stream by name.
    ///
    /// # Errors
    /// [`NatsError::StreamNotFound`] when the server says there is no such stream;
    /// [`NatsError::StreamLookupFailed`] (retryable) when the request did not complete, so
    /// whether it exists is not known; [`NatsError::JetStream`] when the server refused the
    /// lookup.
    #[instrument(skip(self), err(Debug))]
    pub async fn get_stream(&self, name: &str) -> NatsResult<JetStreamStream> {
        let stream = self
            .inner
            .get_stream(name)
            .await
            .map_err(|error| match error.kind() {
                GetStreamErrorKind::JetStream(e)
                    if e.error_code() == ErrorCode::STREAM_NOT_FOUND =>
                {
                    NatsError::StreamNotFound(name.to_owned())
                }
                GetStreamErrorKind::Request => NatsError::StreamLookupFailed {
                    stream: name.to_owned(),
                    reason: error.to_string(),
                },
                _ => NatsError::JetStream(format!("looking up {name}: {error}")),
            })?;
        Ok(JetStreamStream::new(stream, self.wire.prefix().cloned()))
    }

    /// Deletes a stream by name.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if the deletion fails.
    #[instrument(skip(self), err(Debug))]
    pub async fn delete_stream(&self, name: &str) -> NatsResult<()> {
        self.inner
            .delete_stream(name)
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;
        info!(stream = name, "deleted stream");
        Ok(())
    }

    /// Lists summary information for every stream.
    ///
    ///
    /// A stream deleted mid-listing is left out and logged; any other read failure fails the
    /// listing.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if the listing fails, or the error reading a stream.
    #[instrument(skip(self), err(Debug))]
    pub async fn list_streams(&self) -> NatsResult<Vec<StreamInfo>> {
        let mut streams = Vec::new();
        let mut names = self.inner.stream_names();
        while let Some(name) = names.next().await {
            let name = name.map_err(|e| NatsError::JetStream(e.to_string()))?;
            match self.get_stream(&name).await {
                Ok(mut stream) => streams.push(stream.info().await?),
                Err(NatsError::StreamNotFound(_)) => {
                    debug!(stream = %name, "a listed stream was deleted before it was read");
                }
                Err(other) => return Err(other),
            }
        }
        Ok(streams)
    }

    /// Publishes a message to JetStream and awaits its acknowledgment.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if storage fails, or a serialization error.
    #[instrument(skip(self, message), fields(subject = %message.subject()), err(Debug))]
    pub async fn publish<M: Message>(&self, message: M) -> NatsResult<PublishAck> {
        let subject = message.subject().to_string();
        let payload = message.to_bytes()?;
        self.publish_bytes(&subject, payload).await
    }

    /// Publishes an event under the `Nats-Msg-Id` [`EventId::msg_id_for`](crate::EventId::msg_id_for)
    /// names — its id and its payload's digest — and awaits its acknowledgment.
    ///
    /// The broker deduplicates a byte-identical re-publish within the duplicate window.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if storage fails, or a serialization error.
    #[instrument(skip(self, event), fields(subject = %event.subject(), event_id = %event.event_id()), err(Debug))]
    pub async fn publish_event<M: Event>(&self, event: &M) -> NatsResult<PublishAck> {
        let subject = event.subject().to_string();
        let payload = event.to_bytes()?;
        let msg_id = event.event_id().msg_id_for(&payload);
        self.publish_bytes_with_msg_id(&subject, payload, &msg_id)
            .await
    }

    /// Publishes raw bytes to a JetStream subject and awaits acknowledgment.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if storage fails.
    #[instrument(skip(self, payload), fields(subject, bytes = payload.len()), err(Debug))]
    pub async fn publish_bytes(&self, subject: &str, payload: Bytes) -> NatsResult<PublishAck> {
        self.publish_traced(subject, payload, traced_headers())
            .await
    }

    /// Publishes raw bytes to a JetStream subject carrying a `Nats-Msg-Id`, and
    /// awaits acknowledgment.
    ///
    ///
    /// `msg_id` is JetStream's idempotency key: a re-publish within the duplicate window is
    /// answered from the stored copy, with [`PublishAck::duplicate`] set.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if storage fails.
    #[instrument(skip(self, payload), fields(subject, bytes = payload.len(), msg_id), err(Debug))]
    pub async fn publish_bytes_with_msg_id(
        &self,
        subject: &str,
        payload: Bytes,
        msg_id: &str,
    ) -> NatsResult<PublishAck> {
        let mut headers = traced_headers();
        headers.insert(async_nats::header::NATS_MESSAGE_ID.to_string(), msg_id);
        self.publish_traced(subject, payload, headers).await
    }

    /// The one publish body, so every JetStream path carries the trace context.
    async fn publish_traced(
        &self,
        subject: &str,
        payload: Bytes,
        headers: MessageHeaders,
    ) -> NatsResult<PublishAck> {
        // An empty map round-trips as no headers, so this needs no branch.
        let ack = self
            .inner
            // Through the wire, under the namespace the stream declared.
            .publish_with_headers(self.wire.resolve(subject), headers.into(), payload)
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;
        Ok(PublishAck {
            stream: ack.stream,
            sequence: ack.sequence,
            duplicate: ack.duplicate,
        })
    }

    /// Opens a durable consumer and spawns a background loop that runs `handler`
    /// for every message, returning a [`Listener`] guard over that loop.
    ///
    ///
    /// Each message runs in its own span with the propagated trace context. With `auto_ack`, an
    /// `Ok` is acknowledged and an error is logged once here and nak'd for redelivery; without
    /// it, acking is the handler's.
    ///
    /// # Errors
    /// Returns [`NatsError::StreamNotFound`] or [`NatsError::ConsumerNotFound`] if
    /// the stream or consumer is missing, or [`NatsError::JetStream`] if the stream
    /// cannot be opened.
    pub async fn subscribe_consumer_dispatch<M, H, Fut>(
        &self,
        stream_name: &str,
        consumer_name: &str,
        auto_ack: bool,
        handler: H,
    ) -> NatsResult<Listener>
    where
        M: Message,
        H: Fn(ReceivedMessage<M>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = NatsResult<()>> + Send + 'static,
    {
        let stream = self.get_stream(stream_name).await?;
        let consumer = stream.get_consumer(consumer_name).await?;
        let mut subscription = consumer.messages::<M>().await?;
        let stream_name = stream_name.to_string();
        let consumer_name = consumer_name.to_string();
        Ok(Listener::spawn(move |mut drain| async move {
            info!(
                stream = stream_name,
                consumer = consumer_name,
                auto_ack,
                "dispatch loop started"
            );
            loop {
                tokio::select! {
                    biased;
                    () = drain.arrived() => {
                        // Unacknowledged deliveries are redelivered by the server, so nothing is lost.
                        info!(
                            stream = stream_name,
                            consumer = consumer_name,
                            "draining: the message in hand is finished, anything unacknowledged is redelivered"
                        );
                        break;
                    }
                    message = subscription.next() => match message {
                        Some(message) => dispatch_jetstream(&handler, message, auto_ack).await,
                        None => break,
                    },
                }
            }
            info!(
                stream = stream_name,
                consumer = consumer_name,
                "dispatch loop ended; stream closed"
            );
        }))
    }
}

/// Runs `handler` for one JetStream message in its own span, acking or nak'ing it when
/// `auto_ack` is set.
async fn dispatch_jetstream<M, H, Fut>(handler: &H, mut message: ReceivedMessage<M>, auto_ack: bool)
where
    M: Message,
    H: Fn(ReceivedMessage<M>) -> Fut,
    Fut: Future<Output = NatsResult<()>>,
{
    let ids = DeliveryIds::from_headers(&message.headers);
    let span = info_span!(
        "nats.jetstream.message",
        subject = %message.subject,
        request_id = ids.request_id(),
    );
    fundamental_observability::trace_context::adopt_parent(&span, &message.headers);
    async {
        ids.log();
        // With `auto_ack`, the handle is taken so the only ack is this one.
        let ack_handle = if auto_ack {
            message.take_jetstream_message()
        } else {
            None
        };
        match handler(message).await {
            Ok(()) => match ack_handle {
                Some(handle) => match handle.ack().await {
                    Ok(()) => debug!("auto-acked on Ok"),
                    Err(e) => warn!(error = %e, "auto-ack failed; message will be redelivered"),
                },
                None => debug!("handler ok; auto_ack disabled, ack left to the handler"),
            },
            Err(error) => {
                error!(error = %error, error.code = error.error_code(), "handler failed");
                if let Some(handle) = ack_handle {
                    match handle
                        .ack_with(async_nats::jetstream::AckKind::Nak(None))
                        .await
                    {
                        Ok(()) => debug!("nacked after handler error; JetStream will redeliver"),
                        Err(e) => warn!(error = %e, "nack after handler error failed"),
                    }
                }
            }
        }
    }
    .instrument(span)
    .await;
}
