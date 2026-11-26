use std::fmt;
use std::marker::PhantomData;

use bytes::Bytes;
use futures::{FutureExt, StreamExt};
use tracing::{error, instrument, warn};

use super::SubscriptionOptions;
use super::headers::extract_headers;
use crate::error::{NatsError, NatsResult};
use crate::message::{Message, ReceivedMessage};
use crate::subject::{Subject, SubjectPattern, SubjectPrefix};

/// A subscription delivering messages deserialized to `T`; an undecodable one is skipped or ends
/// it per [`SubscriptionOptions::skip_invalid`].
pub struct Subscription<T: Message> {
    inner: async_nats::Subscriber,
    pattern: SubjectPattern,
    options: SubscriptionOptions,
    /// The namespace the connection names subjects under, removed from every delivery.
    prefix: Option<SubjectPrefix>,
    _phantom: PhantomData<fn() -> T>,
}

impl<T: Message> Subscription<T> {
    pub(crate) fn new(
        inner: async_nats::Subscriber,
        pattern: SubjectPattern,
        options: SubscriptionOptions,
        prefix: Option<SubjectPrefix>,
    ) -> Self {
        Self {
            inner,
            pattern,
            options,
            prefix,
            _phantom: PhantomData,
        }
    }

    /// Returns the pattern this subscription matches.
    #[must_use]
    pub fn pattern(&self) -> &SubjectPattern {
        &self.pattern
    }

    /// Unsubscribes, still yielding every delivery the server already routed here; then
    /// [`next`](Self::next) returns `None`.
    ///
    /// # Errors
    /// Returns [`NatsError::Subscription`] if the connection is gone and the unsubscribe
    /// cannot be sent.
    pub async fn drain(&mut self) -> NatsResult<()> {
        self.inner.drain().await.map_err(|e| {
            NatsError::Subscription(format!("could not unsubscribe {}: {e}", self.pattern))
        })
    }

    /// Awaits the next message, or returns `None` once the subscription closes
    /// (or on the first invalid message when `skip_invalid` is `false`).
    #[expect(
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub async fn next(&mut self) -> Option<ReceivedMessage<T>> {
        loop {
            let mut raw = self.inner.next().await?;
            if let Some(prefix) = self.prefix.as_ref() {
                let stripped = prefix.strip(raw.subject.as_str());
                if stripped.len() != raw.subject.len() {
                    raw.subject = stripped.to_owned().into();
                }
            }
            match self.process_message(raw) {
                Ok(msg) => return Some(msg),
                Err(e) if self.options.skip_invalid => {
                    warn!(error = %e, pattern = %self.pattern, "skipping invalid message");
                }
                Err(e) => {
                    error!(error = %e, pattern = %self.pattern, "closing subscription on invalid message");
                    return None;
                }
            }
        }
    }

    /// Returns a ready message without awaiting, or `None` if none is buffered or
    /// the subscription has closed.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn try_next(&mut self) -> Option<ReceivedMessage<T>> {
        loop {
            match self.inner.next().now_or_never() {
                None | Some(None) => return None,
                Some(Some(raw)) => match self.process_message(raw) {
                    Ok(msg) => return Some(msg),
                    Err(e) if self.options.skip_invalid => {
                        warn!(error = %e, pattern = %self.pattern, "skipping invalid message");
                    }
                    Err(e) => {
                        error!(error = %e, pattern = %self.pattern, "closing subscription on invalid message");
                        return None;
                    }
                },
            }
        }
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

    fn process_message(&self, raw: async_nats::Message) -> NatsResult<ReceivedMessage<T>> {
        let subject = Subject::new_unchecked(raw.subject.as_str());
        let reply = raw
            .reply
            .as_ref()
            .map(|s| Subject::new_unchecked(s.as_str()));
        let headers = extract_headers(&raw);
        let bytes = Bytes::copy_from_slice(&raw.payload);
        let payload = T::from_bytes(&raw.payload)?;
        Ok(ReceivedMessage::new(
            payload, subject, reply, headers, bytes,
        ))
    }
}

impl<T: Message> fmt::Debug for Subscription<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription")
            .field("pattern", &self.pattern)
            .field("options", &self.options)
            .field("message_type", &std::any::type_name::<T>())
            .finish()
    }
}
