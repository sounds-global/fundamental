//! [`OutboxRow`]: the contract a model implements to be drained as a transactional-outbox entry.

use std::borrow::Cow;

use chrono::{DateTime, Utc};
use fundamental_core::model::{BoundTo, Database, HasLeadingKey, Model};
use fundamental_core::outbox::SealedOutbox;

use crate::QueryBuilder;

/// A model that can be relayed by the outbox [`drain_pending`](super::drain_pending)
/// loop.
///
/// The row supplies its own query and bookkeeping, so the drain knows no column names; the
/// side effect and the mark-relayed transition live in the caller's [`Relay`](super::Relay).
///
/// The next-attempt column serves both the claim's lease and the failure backoff
/// ([`RetryPolicy`](super::RetryPolicy)); give it `NOT NULL DEFAULT now()`. A row is a
/// [`SealedOutbox`], so a row written without the key refuses to decode and is not relayed.
pub trait OutboxRow<D>: Model + SealedOutbox + BoundTo<D> + Send + Sync
where
    D: Database,
{
    /// A stable idempotency key for this entry.
    ///
    /// Logged on every relay, the `Nats-Msg-Id` for a JetStream relay, and the seed of the retry
    /// jitter.
    fn msg_id(&self) -> Cow<'_, str>;

    /// How many relay attempts this row has already recorded.
    ///
    /// The counter [`note_failure`](Self::note_failure) increments.
    fn attempts(&self) -> i64;

    /// What one claim is scoped to.
    ///
    /// `()` for a single-column key; for a key that leads with a tenancy, that tenancy's value,
    /// taken from boot configuration and never from a request.
    type Scope: Send + Sync + core::fmt::Debug;

    /// The rows of this table still waiting to be relayed, within `scope`.
    ///
    /// Only the done column `IS NULL` and the tenancy; the claim adds due-ness, ordering and the
    /// limit. The return type is already addressable because a claim ends in a locked read, and a
    /// lock on a key prefix would span tenancies.
    fn pending(scope: &Self::Scope) -> QueryBuilder<Self, D, HasLeadingKey>
    where
        Self: Sized;

    /// The next-attempt column, `NOT NULL`: when this row may next be attempted.
    ///
    /// Compared with a `now` passed in, which keeps the retry schedule testable.
    fn next_attempt_column() -> &'static str
    where
        Self: Sized;

    /// The column holding when this row is **due**, for a table whose rows are scheduled for a
    /// chosen time rather than relayed as soon as they are written; `None`, the default, for one
    /// that relays at once.
    fn due_column() -> Option<&'static str>
    where
        Self: Sized,
    {
        None
    }

    /// Sets the earliest time this row may next be attempted.
    ///
    /// Called by the claimer with a lease and by the drain with a backoff.
    fn defer_until(&mut self, at: DateTime<Utc>);

    /// Records that a relay attempt failed, so the row stays pending and visible.
    ///
    /// Bumps the attempt counter and stores `error`; the drain then defers and persists the row.
    fn note_failure(&mut self, error: &str);
}
