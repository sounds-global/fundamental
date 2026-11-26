//! [`Dispatch`] — one listener's loop: bounded concurrency, one transaction per delivery, and a
//! settlement for every delivery it takes.

use std::collections::HashSet;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use fundamental_core::Error as _;
use fundamental_core::retry::OsEntropy;
use fundamental_db::DatabaseError;
use fundamental_nats::{
    DrainRequested, Event, EventId, JetStreamDeliveries, JetStreamDelivery, PayloadDigest,
};
use fundamental_sql::SqlPool;
use futures::stream::FuturesUnordered;
use futures::{FutureExt as _, StreamExt as _};
use tokio::sync::Notify;
use tokio::time::{MissedTickBehavior, interval_at, sleep};
use tracing::field::Empty;
use tracing::{Instrument as _, Span, debug, error, info, info_span, warn};

use crate::park::park;
use crate::process::process;
use crate::settlement::Settlement;
use crate::verdict::Verdict;
use crate::watermark::Watermark;
use crate::{Binding, Inbound, InboxConfig, InboxError, ParkReason, Step};

/// How long a second copy of an event already in flight on this listener waits before it is
/// delivered again — long enough for the first to commit, so the copy is then a duplicate.
const IN_FLIGHT_DELAY: Duration = Duration::from_secs(1);

/// How long a delivery waits when its listener cannot read the effect's watermark.
const UNREAD_WATERMARK_DELAY: Duration = Duration::from_secs(5);

type Settled = Pin<Box<dyn Future<Output = Option<EventId>> + Send>>;

/// Everything a listener's deliveries share.
pub(crate) struct Dispatch<M, E> {
    pub(crate) pool: SqlPool,
    pub(crate) config: InboxConfig,
    pub(crate) binding: Binding,
    pub(crate) step: Step<M, E>,
    /// When the bound stream was created, which with a sequence names one stored message.
    pub(crate) stream_created_at: DateTime<Utc>,
    /// How often an effect in progress tells the server so: a third of the consumer's ack wait.
    pub(crate) heartbeat: Duration,
    /// The latest reading of the effect's watermark. The lock is never held across an await.
    pub(crate) watermark: Mutex<Option<Watermark>>,
}

/// A delivery that decoded and named itself, or one settled without a transaction.
enum Prepared<M> {
    Ready(JetStreamDelivery, Inbound<M>, PayloadDigest),
    Refused(JetStreamDelivery, Option<EventId>, Verdict),
}

impl<M, E> Dispatch<M, E>
where
    M: Event + Send + Sync + 'static,
    E: fundamental_core::Error + From<DatabaseError> + Send + 'static,
{
    /// Takes deliveries until the stream ends or a drain is asked for, keeping at most
    /// `concurrency` in flight, then finishes every one in flight.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) async fn run(
        self: Arc<Self>,
        mut deliveries: JetStreamDeliveries,
        mut drain: DrainRequested,
    ) {
        let effect = self.binding.effect().as_str();
        let concurrency = self.config.concurrency();
        let mut in_flight: FuturesUnordered<Settled> = FuturesUnordered::new();
        let mut holding: HashSet<EventId> = HashSet::new();
        loop {
            tokio::select! {
                biased;
                () = drain.arrived() => {
                    info!(effect, in_flight = in_flight.len(), "inbox draining: finishing what is in flight");
                    break;
                }
                Some(released) = in_flight.next(), if !in_flight.is_empty() => {
                    if let Some(id) = released {
                        holding.remove(&id);
                    }
                }
                next = deliveries.next(), if in_flight.len() < concurrency => {
                    let Some(delivery) = next else {
                        info!(effect, "inbox consumer closed");
                        break;
                    };
                    match self.prepare(delivery) {
                        Prepared::Refused(delivery, event_id, verdict) => {
                            in_flight.push(Box::pin(Arc::clone(&self).refused(delivery, event_id, verdict)));
                        }
                        Prepared::Ready(delivery, inbound, digest) => {
                            let id = inbound.event_id();
                            if holding.insert(id) {
                                in_flight.push(Box::pin(Arc::clone(&self).apply(delivery, inbound, digest)));
                            } else {
                                in_flight.push(Box::pin(shed(effect, delivery)));
                            }
                        }
                    }
                }
            }
        }
        while in_flight.next().await.is_some() {}
        info!(effect, "inbox listener ended");
    }

    /// Decodes a delivery and checks it names the event it carries, before any transaction.
    fn prepare(&self, delivery: JetStreamDelivery) -> Prepared<M> {
        let payload = match delivery.decode::<M>() {
            Ok(payload) => payload,
            Err(error) => {
                let detail = error.to_string();
                return Prepared::Refused(delivery, None, Verdict::Undecodable { detail });
            }
        };
        let id = payload.event_id();
        if !id.is_v7() {
            return Prepared::Refused(
                delivery,
                Some(id),
                Verdict::Refused {
                    reason: ParkReason::InvalidEventId,
                    detail: "the event id is not a UUIDv7".to_owned(),
                },
            );
        }
        if !delivery.names_event(id) {
            return Prepared::Refused(
                delivery,
                Some(id),
                Verdict::Refused {
                    reason: ParkReason::InvalidEventId,
                    detail: "its Nats-Msg-Id does not name this event and payload; publish it with publish_event".to_owned(),
                },
            );
        }
        let digest = PayloadDigest::derive(&[&delivery.payload]);
        let inbound = Inbound::new(
            payload,
            id,
            delivery.subject.clone(),
            delivery.delivery.clone(),
        );
        Prepared::Ready(delivery, inbound, digest)
    }

    /// Settles a delivery decided before its transaction.
    async fn refused(
        self: Arc<Self>,
        delivery: JetStreamDelivery,
        event_id: Option<EventId>,
        verdict: Verdict,
    ) -> Option<EventId> {
        let span = self.span(&delivery);
        async {
            self.settle(delivery, event_id, &verdict, Instant::now())
                .await;
        }
        .instrument(span)
        .await;
        None
    }

    /// Applies one delivery's effect under a deadline, a heartbeat and a panic guard, then
    /// settles it. Returns the event id, which this listener stops holding.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn apply(
        self: Arc<Self>,
        delivery: JetStreamDelivery,
        inbound: Inbound<M>,
        digest: PayloadDigest,
    ) -> Option<EventId> {
        let span = self.span(&delivery);
        fundamental_observability::trace_context::adopt_parent(&span, &delivery.headers);
        let id = inbound.event_id();
        async {
            let started = Instant::now();
            let Some(watermark) = self.watermark().await else {
                let delay_ms = u64::try_from(UNREAD_WATERMARK_DELAY.as_millis()).unwrap_or(u64::MAX);
                warn!(event = "inbox.retry", delay_ms, "the effect's watermark could not be read; delivering again");
                if let Err(error) = delivery.nak(UNREAD_WATERMARK_DELAY).await {
                    warn!(%error, "the redelivery request was not sent; the ack wait will redeliver it");
                }
                return;
            };
            let began = Notify::new();
            let work = AssertUnwindSafe(process(&self, &inbound, digest, watermark, &began))
            .catch_unwind();
            tokio::pin!(work);
            // The deadline starts once the transaction holds a connection; waiting for one is
            // bounded by the pool's acquire timeout.
            let deadline = async {
                began.notified().await;
                sleep(self.config.effect_deadline()).await;
            };
            tokio::pin!(deadline);
            let mut beat = interval_at(
                tokio::time::Instant::now() + self.heartbeat,
                self.heartbeat,
            );
            beat.set_missed_tick_behavior(MissedTickBehavior::Delay);
            let verdict = loop {
                tokio::select! {
                    biased;
                    outcome = &mut work => break outcome.unwrap_or(Verdict::Panicked),
                    () = &mut deadline => break Verdict::DeadlineExceeded,
                    _ = beat.tick() => {
                        if let Err(error) = delivery.in_progress().await {
                            warn!(%error, "the in-progress signal was not sent; the delivery may be sent again");
                        }
                    }
                }
            };
            self.settle(delivery, Some(id), &verdict, started).await;
        }
        .instrument(span)
        .await;
        Some(id)
    }

    /// The effect's watermark, read again when the last reading is due; `None` when it cannot
    /// be read, in which case nothing may be applied.
    async fn watermark(&self) -> Option<Watermark> {
        let cached = *self
            .watermark
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(reading) = cached
            && !reading.due()
        {
            return Some(reading);
        }
        match crate::inbox::read_watermark(&self.pool, self.binding.effect()).await {
            Ok(reading) => {
                *self
                    .watermark
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(reading);
                Some(reading)
            }
            Err(error) => {
                warn!(
                    effect = %self.binding.effect(),
                    code = error.error_code(),
                    %error,
                    "the effect's watermark could not be read"
                );
                None
            }
        }
    }

    /// Answers a delivery as its verdict decides, logging the one line that says what happened.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    async fn settle(
        &self,
        delivery: JetStreamDelivery,
        event_id: Option<EventId>,
        verdict: &Verdict,
        started: Instant,
    ) {
        let settlement = Settlement::decide(
            verdict,
            delivery.delivery.delivered,
            &self.config,
            &mut OsEntropy,
        );
        let effect = self.binding.effect().as_str();
        let span = Span::current();
        span.record("outcome", verdict.outcome());
        span.record("settle", settlement.name());
        span.record(
            "elapsed_ms",
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        );
        match settlement {
            Settlement::Ack => {
                match verdict {
                    Verdict::Duplicate => {
                        debug!(event = "inbox.duplicate", event_id = ?event_id, "already applied; acknowledged")
                    }
                    _ => {
                        debug!(event = "inbox.processed", event_id = ?event_id, "applied and acknowledged")
                    }
                }
                if let Err(error) = delivery.ack().await {
                    warn!(event = "inbox.ack_failed", %error, "the acknowledgement was not sent; the redelivery will be a duplicate");
                }
            }
            Settlement::Nak(delay) => {
                let delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX);
                match verdict {
                    Verdict::CommitUnknown => warn!(
                        event = "inbox.commit_unknown",
                        delay_ms, "the commit's answer was lost; the redelivery will find out"
                    ),
                    Verdict::Failed { code, retryable } => {
                        span.record("code", code.as_str());
                        warn!(
                            event = "inbox.retry",
                            code,
                            retryable,
                            delay_ms,
                            "the effect failed and rolled back; delivering again"
                        );
                    }
                    Verdict::Undecodable { detail } => {
                        let error = InboxError::Undecodable {
                            effect: effect.to_owned(),
                            detail: detail.clone(),
                        };
                        span.record("code", error.error_code());
                        warn!(event = "inbox.retry", code = error.error_code(), error = %diagnosis(&error), delay_ms, "the payload did not decode; delivering again for a replica that reads it");
                    }
                    Verdict::Panicked => {
                        let error = InboxError::HandlerPanicked {
                            effect: effect.to_owned(),
                        };
                        span.record("code", error.error_code());
                        error!(
                            event = "inbox.handler_panicked",
                            code = error.error_code(),
                            delay_ms,
                            "the handler panicked; its transaction was rolled back"
                        );
                    }
                    Verdict::DeadlineExceeded => {
                        let error = self.deadline_error();
                        span.record("code", error.error_code());
                        warn!(event = "inbox.retry", code = error.error_code(), error = %diagnosis(&error), delay_ms, "delivering again");
                    }
                    other => warn!(
                        event = "inbox.retry",
                        outcome = other.outcome(),
                        delay_ms,
                        "delivering again"
                    ),
                }
                if let Err(error) = delivery.nak(delay).await {
                    warn!(%error, "the redelivery request was not sent; the ack wait will redeliver it");
                }
            }
            Settlement::Park { reason, code } => {
                let detail = match verdict {
                    Verdict::Refused { detail, .. } | Verdict::Undecodable { detail } => {
                        detail.clone()
                    }
                    Verdict::DeadlineExceeded => diagnosis(&self.deadline_error()),
                    _ => String::new(),
                };
                let parked = park(
                    &self.pool,
                    self.binding.effect(),
                    self.stream_created_at,
                    &delivery.delivery,
                    event_id,
                    reason,
                    code.as_deref(),
                )
                .await;
                match parked {
                    Ok(()) => {
                        span.record("code", code.as_deref().unwrap_or_default());
                        error!(
                            event = "inbox.parked",
                            reason = reason.as_str(),
                            code = code.as_deref(),
                            detail,
                            "refused and recorded for an operator; terminated"
                        );
                        if let Err(error) = delivery.term().await {
                            warn!(%error, "the termination was not sent; the redelivery will find its record and be terminated");
                        }
                    }
                    Err(park_error) => {
                        let delay = self
                            .config
                            .retry_delay(delivery.delivery.delivered, &mut OsEntropy);
                        error!(
                            event = "inbox.park_failed",
                            reason = reason.as_str(),
                            code = park_error.error_code(),
                            error = %park_error,
                            "the refusal could not be recorded; delivering again rather than ending it untraced"
                        );
                        if let Err(error) = delivery.nak(delay).await {
                            warn!(%error, "the redelivery request was not sent; the ack wait will redeliver it");
                        }
                    }
                }
            }
        }
    }

    fn deadline_error(&self) -> InboxError {
        InboxError::DeadlineExceeded {
            effect: self.binding.effect().as_str().to_owned(),
            deadline_secs: self.config.effect_deadline().as_secs(),
        }
    }

    fn span(&self, delivery: &JetStreamDelivery) -> Span {
        info_span!(
            "inbox.process",
            effect = %self.binding.effect(),
            stream = self.binding.stream(),
            consumer = self.binding.consumer(),
            stream_sequence = delivery.delivery.stream_sequence,
            delivered = delivery.delivery.delivered,
            outcome = Empty,
            settle = Empty,
            code = Empty,
            elapsed_ms = Empty,
        )
    }
}

/// The operator's account of `error`: its developer message, or its user-safe one without it.
fn diagnosis(error: &InboxError) -> String {
    error
        .developer_message()
        .map_or_else(|| error.to_string(), std::borrow::Cow::into_owned)
}

/// Sends a second in-flight copy of an event back for later, without opening a transaction.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn shed(effect: &'static str, delivery: JetStreamDelivery) -> Option<EventId> {
    debug!(
        effect,
        stream_sequence = delivery.delivery.stream_sequence,
        "a copy of an event already in flight; delivering it again shortly"
    );
    if let Err(error) = delivery.nak(IN_FLIGHT_DELAY).await {
        warn!(effect, %error, "the redelivery request was not sent; the ack wait will redeliver it");
    }
    None
}
