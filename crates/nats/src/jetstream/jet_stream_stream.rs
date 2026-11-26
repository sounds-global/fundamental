use async_nats::jetstream;
use async_nats::jetstream::consumer::pull::Config as PullConfig;
use async_nats::jetstream::context::{ConsumerInfoError, ConsumerInfoErrorKind};
use tracing::{info, instrument};

use super::{ConsumerConfig, JetStreamConsumer, StreamInfo};
use crate::error::{NatsError, NatsResult};
use crate::subject::SubjectPrefix;

/// A handle to a JetStream stream, used to inspect it and manage its consumers.
#[derive(Debug, Clone)]
pub struct JetStreamStream {
    inner: jetstream::stream::Stream,
    /// This connection's namespace, stripped from each delivery's subject.
    prefix: Option<SubjectPrefix>,
}

impl JetStreamStream {
    pub(crate) fn new(inner: jetstream::stream::Stream, prefix: Option<SubjectPrefix>) -> Self {
        Self { inner, prefix }
    }

    /// Fetches current stream information.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if the request fails.
    #[instrument(skip(self), err(Debug))]
    pub async fn info(&mut self) -> NatsResult<StreamInfo> {
        let info = self
            .inner
            .info()
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;
        Ok(StreamInfo::from_jetstream(info, self.prefix.as_ref()))
    }

    /// The configuration this handle was fetched with, as the server reported it.
    pub(crate) fn cached_config(&self) -> jetstream::stream::Config {
        self.inner.cached_info().config.clone()
    }

    /// Creates (or updates) a pull consumer on this stream.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if creation fails.
    #[instrument(skip(self, config), fields(consumer = %config.name), err(Debug))]
    pub async fn create_consumer(&self, config: ConsumerConfig) -> NatsResult<JetStreamConsumer> {
        let name = config.name.clone();
        let consumer: jetstream::consumer::Consumer<PullConfig> = self
            .inner
            .create_consumer(config.into_jetstream_config())
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;
        info!(consumer = %name, "created consumer");
        Ok(JetStreamConsumer::new(consumer, self.prefix.clone()))
    }

    /// Looks up an existing pull consumer.
    ///
    /// # Errors
    /// [`NatsError::ConsumerNotFound`] when the server says there is no such consumer;
    /// [`NatsError::ConsumerLookupFailed`] (retryable) when the request did not complete;
    /// [`NatsError::JetStream`] for any other refusal.
    #[instrument(skip(self), err(Debug))]
    pub async fn get_consumer(&self, name: &str) -> NatsResult<JetStreamConsumer> {
        let consumer: jetstream::consumer::Consumer<PullConfig> =
            self.inner.get_consumer(name).await.map_err(|error| {
                match error
                    .downcast_ref::<ConsumerInfoError>()
                    .map(ConsumerInfoError::kind)
                {
                    Some(ConsumerInfoErrorKind::NotFound) => {
                        NatsError::ConsumerNotFound(name.to_owned())
                    }
                    Some(
                        ConsumerInfoErrorKind::TimedOut
                        | ConsumerInfoErrorKind::Request
                        | ConsumerInfoErrorKind::NoResponders
                        | ConsumerInfoErrorKind::Offline,
                    ) => NatsError::ConsumerLookupFailed {
                        consumer: name.to_owned(),
                        reason: error.to_string(),
                    },
                    _ => NatsError::JetStream(format!("looking up consumer {name}: {error}")),
                }
            })?;
        Ok(JetStreamConsumer::new(consumer, self.prefix.clone()))
    }

    /// Deletes a consumer by name.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if the deletion fails.
    #[instrument(skip(self), err(Debug))]
    pub async fn delete_consumer(&self, name: &str) -> NatsResult<()> {
        self.inner
            .delete_consumer(name)
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;
        info!(consumer = name, "deleted consumer");
        Ok(())
    }

    /// Purges all messages from the stream, returning the count removed.
    ///
    /// # Errors
    /// Returns [`NatsError::JetStream`] if the purge fails.
    #[instrument(skip(self), err(Debug))]
    pub async fn purge(&self) -> NatsResult<u64> {
        let response = self
            .inner
            .purge()
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;
        info!(purged = response.purged, "purged stream");
        Ok(response.purged)
    }
}
