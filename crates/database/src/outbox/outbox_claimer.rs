//! [`OutboxClaimer`]: take exclusive ownership of a batch of outbox rows.

use std::future::Future;

use chrono::{DateTime, Utc};
use fundamental_core::AnyError;
use fundamental_core::model::Database;

use super::{OutboxClaim, OutboxRow};

/// Claims up to `batch` rows that are due, marking them not-due for the lease so no
/// other worker takes them.
///
/// The claim is a short transaction that moves each row's next attempt to `lease_until` and
/// commits, never a lock held across the relay's I/O; a dead worker's lease simply expires.
/// Implementations must make it mutually exclusive (on YSQL, `FOR UPDATE SKIP LOCKED`). A
/// claim returns some due rows, not necessarily all: a row written a moment ago may be missed
/// until the next one.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot claim `{R}`'s outbox rows",
    label = "no claimer for this outbox row",
    note = "a claim ends in a locked read and a batched lease write, both addressed by the row's \
            whole key. If `{R}` is keyed by more than one column, check that its `pending` \
            pins the leading key column with `pin_key(scope)` — a claim that named no tenancy \
            would take work belonging to every one of them."
)]
pub trait OutboxClaimer<R, D>: Send + Sync
where
    R: OutboxRow<D>,
    D: Database,
{
    /// Claims up to `batch` rows due at `now` within `scope`, deferring each to `lease_until`.
    ///
    /// Returns the claimed rows, already deferred, and what the claim did with every due row it
    /// could not decode. `scope` is [`OutboxRow::Scope`] and bounds both the read and the lease
    /// write.
    ///
    /// # Errors
    /// Returns [`AnyError`] if the batch could not be claimed; nothing was taken.
    fn claim(
        &self,
        scope: &R::Scope,
        batch: u64,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> impl Future<Output = Result<OutboxClaim<R>, AnyError>> + Send;

    /// Persists a failed relay's bookkeeping on `row` — its attempts, its error and its next
    /// attempt — only while the stored row is still pending within `scope` and still carries the
    /// lease `leased_until` that this worker's claim wrote.
    ///
    /// Returns `false`, having written nothing, when the row moved on while it was being relayed
    /// (relayed after all, rescheduled, or claimed again), so a stale snapshot never undoes it.
    ///
    /// # Errors
    /// Returns [`AnyError`] if the write fails.
    fn record_failure(
        &self,
        scope: &R::Scope,
        row: &R,
        leased_until: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, AnyError>> + Send;
}
