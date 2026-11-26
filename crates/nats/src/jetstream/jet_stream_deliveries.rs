use async_nats::jetstream;
use futures::StreamExt;
use tracing::error;

use super::JetStreamDelivery;
use crate::subject::SubjectPrefix;

/// The deliveries of a pull consumer, each an undecoded [`JetStreamDelivery`].
pub struct JetStreamDeliveries {
    inner: jetstream::consumer::pull::Stream,
    prefix: Option<SubjectPrefix>,
}

impl JetStreamDeliveries {
    pub(crate) fn new(
        inner: jetstream::consumer::pull::Stream,
        prefix: Option<SubjectPrefix>,
    ) -> Self {
        Self { inner, prefix }
    }

    /// Awaits the next delivery, or `None` once the stream ends.
    ///
    /// A failed pull, or a message with no delivery metadata, is logged and skipped; the server
    /// redelivers it after its ack wait.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub async fn next(&mut self) -> Option<JetStreamDelivery> {
        loop {
            match self.inner.next().await? {
                Ok(message) => match JetStreamDelivery::new(message, self.prefix.as_ref()) {
                    Ok(delivery) => return Some(delivery),
                    Err(e) => {
                        error!(error = %e, "skipping a JetStream message with no delivery metadata")
                    }
                },
                Err(e) => error!(error = %e, "error receiving a JetStream delivery"),
            }
        }
    }
}

impl std::fmt::Debug for JetStreamDeliveries {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JetStreamDeliveries")
            .finish_non_exhaustive()
    }
}
