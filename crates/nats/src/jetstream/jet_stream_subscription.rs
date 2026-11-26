use std::fmt;
use std::marker::PhantomData;
use std::time::Duration;

use async_nats::jetstream::{self, AckKind};
use bytes::Bytes;
use fundamental_core::retry::{Backoff, OsEntropy};
use futures::StreamExt;
use tracing::error;

use crate::message::{Message, ReceivedMessage};
use crate::subject::{Subject, SubjectPrefix};
use crate::subscription::headers::extract_headers;

/// A stream of durable messages deserialized to `M`. Each [`ReceivedMessage`]
/// carries its JetStream handle so the caller can `ack`/`nack` it.
pub struct JetStreamSubscription<M: Message> {
    inner: jetstream::consumer::pull::Stream,
    /// This connection's namespace, stripped from each delivery's subject.
    prefix: Option<SubjectPrefix>,
    _phantom: PhantomData<fn() -> M>,
}

impl<M: Message> JetStreamSubscription<M> {
    pub(crate) fn new(
        inner: jetstream::consumer::pull::Stream,
        prefix: Option<SubjectPrefix>,
    ) -> Self {
        Self {
            inner,
            prefix,
            _phantom: PhantomData,
        }
    }

    /// Awaits the next message, or `None` once the stream ends. An undecodable message is
    /// redelivered after a growing, jittered delay; a failed pull is logged and skipped.
    #[expect(
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub async fn next(&mut self) -> Option<ReceivedMessage<M>> {
        loop {
            match self.inner.next().await? {
                Ok(msg) => {
                    let subject = Subject::new_unchecked(
                        self.prefix.as_ref().map_or(msg.subject.as_str(), |prefix| {
                            prefix.strip(msg.subject.as_str())
                        }),
                    );
                    match M::from_bytes(&msg.payload) {
                        Ok(payload) => {
                            let raw = Bytes::copy_from_slice(&msg.payload);
                            let headers = extract_headers(&msg);
                            return Some(
                                ReceivedMessage::new(payload, subject, None, headers, raw)
                                    .with_jetstream_message(msg),
                            );
                        }
                        // Not terminated: in a rolling deploy a newer replica may decode it.
                        Err(e) => {
                            let delay = undecodable_delay(&msg);
                            match msg.ack_with(AckKind::Nak(Some(delay))).await {
                                Ok(()) => error!(
                                    error = %e,
                                    subject = subject.as_str(),
                                    delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                                    "a JetStream message did not decode; delivering it again later"
                                ),
                                Err(nak) => error!(
                                    error = %e,
                                    nak_error = %nak,
                                    subject = subject.as_str(),
                                    "a JetStream message did not decode and could not be sent back; the ack wait will redeliver it"
                                ),
                            }
                        }
                    }
                }
                Err(e) => error!(error = %e, "error receiving JetStream message"),
            }
        }
    }
}

impl<M: Message> fmt::Debug for JetStreamSubscription<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JetStreamSubscription")
            .field("message_type", &std::any::type_name::<M>())
            .finish()
    }
}

/// No redelivery of a message that did not decode comes sooner than this.
const UNDECODABLE_FLOOR: Duration = Duration::from_secs(5);

/// The first redelivery of a message that did not decode is spread across this window.
const UNDECODABLE_FIRST: Duration = Duration::from_secs(30);

/// No redelivery of an undecodable message waits longer than this.
const UNDECODABLE_CAP: Duration = Duration::from_secs(600);

/// How long before an undecodable message is delivered again: the floor plus a full-jitter
/// window doubling per attempt up to the cap.
fn undecodable_delay(message: &jetstream::Message) -> Duration {
    let step = message.info().map_or(0, |info| {
        u32::try_from(info.delivered.saturating_sub(1)).unwrap_or(u32::MAX)
    });
    match Backoff::new(UNDECODABLE_FIRST, 2.0, UNDECODABLE_CAP) {
        Ok(backoff) => backoff.draw(step, Some(UNDECODABLE_FLOOR), &mut OsEntropy),
        Err(_) => UNDECODABLE_CAP,
    }
}
