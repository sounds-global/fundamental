use std::fmt;

use futures::StreamExt;
use tracing::instrument;

use super::RawReceivedMessage;
use crate::error::{NatsError, NatsResult};
use crate::subject::{SubjectPattern, SubjectPrefix};

/// A subscription yielding raw bytes, for mixed message types on one subject.
pub struct RawSubscription {
    inner: async_nats::Subscriber,
    pattern: SubjectPattern,
    /// The namespace the connection names subjects under, removed from every delivery.
    prefix: Option<SubjectPrefix>,
}

impl RawSubscription {
    pub(crate) fn new(
        inner: async_nats::Subscriber,
        pattern: SubjectPattern,
        prefix: Option<SubjectPrefix>,
    ) -> Self {
        Self {
            inner,
            pattern,
            prefix,
        }
    }

    /// Returns the pattern this subscription matches.
    #[must_use]
    pub fn pattern(&self) -> &SubjectPattern {
        &self.pattern
    }

    /// Returns the next message, or `None` once the subscription closes.
    pub async fn next(&mut self) -> Option<RawReceivedMessage> {
        let mut raw = self.inner.next().await?;
        if let Some(prefix) = self.prefix.as_ref() {
            let stripped = prefix.strip(raw.subject.as_str());
            if stripped.len() != raw.subject.len() {
                raw.subject = stripped.to_owned().into();
            }
        }
        Some(RawReceivedMessage::from(raw))
    }

    /// Unsubscribes, dropping the subscription.
    ///
    /// # Errors
    /// Returns [`NatsError::Subscription`] if the unsubscribe request fails.
    #[instrument(skip(self), fields(pattern = %self.pattern), err(Debug))]
    pub async fn unsubscribe(mut self) -> NatsResult<()> {
        self.inner
            .unsubscribe()
            .await
            .map_err(|e| NatsError::subscription(e.to_string()))
    }
}

impl fmt::Debug for RawSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawSubscription")
            .field("pattern", &self.pattern)
            .finish()
    }
}
