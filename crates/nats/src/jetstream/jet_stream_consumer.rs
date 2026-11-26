use async_nats::jetstream;
use async_nats::jetstream::consumer::pull::Config as PullConfig;
use bytes::Bytes;
use futures::StreamExt;
use tracing::instrument;

use super::{AckPolicy, ConsumerInfo, JetStreamDeliveries, JetStreamSubscription};
use crate::error::{NatsError, NatsResult};
use crate::message::{Message, ReceivedMessage};
use crate::subject::{Subject, SubjectPrefix};
use crate::subscription::headers::extract_headers;

/// A raw JetStream pull stream, for consumers spanning heterogeneous subjects.
pub type RawMessageStream = jetstream::consumer::pull::Stream;

/// A JetStream pull consumer: stream typed messages, fetch batches, or read raw.
#[derive(Debug, Clone)]
pub struct JetStreamConsumer {
    inner: jetstream::consumer::Consumer<PullConfig>,
    /// This connection's namespace, stripped from each delivery's subject.
    prefix: Option<SubjectPrefix>,
}

impl JetStreamConsumer {
    pub(crate) fn new(
        inner: jetstream::consumer::Consumer<PullConfig>,
        prefix: Option<SubjectPrefix>,
    ) -> Self {
        Self { inner, prefix }
    }

    /// Fetches current consumer information.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if the request fails.
    #[instrument(skip(self), err(Debug))]
    pub async fn info(&mut self) -> NatsResult<ConsumerInfo> {
        let info = self
            .inner
            .info()
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;
        Ok(ConsumerInfo {
            name: info.name.clone(),
            stream: info.stream_name.clone(),
            num_pending: info.num_pending,
            num_waiting: info.num_waiting,
            num_ack_pending: info.num_ack_pending,
            num_redelivered: info.num_redelivered,
            ack_policy: AckPolicy::from(info.config.ack_policy),
            max_deliver: info.config.max_deliver,
            ack_wait: info.config.ack_wait,
        })
    }

    /// Opens a typed message stream for this consumer.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if the stream cannot be opened.
    #[instrument(skip(self), err(Debug))]
    pub async fn messages<M: Message>(&self) -> NatsResult<JetStreamSubscription<M>> {
        let messages = self
            .inner
            .messages()
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;
        Ok(JetStreamSubscription::new(messages, self.prefix.clone()))
    }

    /// Opens a raw message stream, leaving deserialization to the caller.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if the stream cannot be opened.
    #[instrument(skip(self), err(Debug))]
    pub async fn raw_messages(&self) -> NatsResult<RawMessageStream> {
        self.inner
            .messages()
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))
    }

    /// Opens this consumer's deliveries undecoded, for a caller that settles each itself.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if the stream cannot be opened.
    #[instrument(skip(self), err(Debug))]
    pub async fn deliveries(&self) -> NatsResult<JetStreamDeliveries> {
        let messages = self
            .inner
            .messages()
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;
        Ok(JetStreamDeliveries::new(messages, self.prefix.clone()))
    }

    /// Fetches up to `batch` messages.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if the fetch fails, or a serialization error.
    #[instrument(skip(self), err(Debug))]
    pub async fn fetch<M: Message>(&self, batch: usize) -> NatsResult<Vec<ReceivedMessage<M>>> {
        let mut messages = self
            .inner
            .fetch()
            .max_messages(batch)
            .messages()
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;

        let mut result = Vec::with_capacity(batch);
        while let Some(msg) = messages.next().await {
            let msg = msg.map_err(|e| NatsError::JetStream(e.to_string()))?;
            let subject = Subject::new_unchecked(
                self.prefix.as_ref().map_or(msg.subject.as_str(), |prefix| {
                    prefix.strip(msg.subject.as_str())
                }),
            );
            let payload = M::from_bytes(&msg.payload)?;
            let raw = Bytes::copy_from_slice(&msg.payload);
            let headers = extract_headers(&msg);
            result.push(
                ReceivedMessage::new(payload, subject, None, headers, raw)
                    .with_jetstream_message(msg),
            );
        }
        Ok(result)
    }
}
