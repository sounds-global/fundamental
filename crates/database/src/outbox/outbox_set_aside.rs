//! [`OutboxSetAside`]: what an outbox table holds that its relay will not send, and putting it back.

use core::future::Future;

use chrono::{DateTime, Utc};
use fundamental_core::AnyError;
use fundamental_core::model::Database;
use fundamental_core::outbox::OutboxInventory;

use super::{OutboxRow, RetryPolicy};

/// Counts what an outbox table holds that its relay will not send on its own, and re-queues the
/// rows a claim set aside.
pub trait OutboxSetAside<R, D>: Send + Sync
where
    R: OutboxRow<D>,
    D: Database,
{
    /// What `scope`'s share of the table holds at `now`: rows set aside, by reason; waiting rows
    /// deferred past `policy`'s [`deferral_horizon`](RetryPolicy::deferral_horizon); and waiting
    /// rows a claim skipped.
    ///
    /// Every count stops at `cap` and says so. A set-aside counts under its reason only when its
    /// seal checks.
    ///
    /// # Errors
    /// Returns [`AnyError`] when the store cannot be read, or when this process holds no key
    /// manager to check a set-aside seal with.
    fn inventory(
        &self,
        scope: &R::Scope,
        now: DateTime<Utc>,
        policy: RetryPolicy,
        cap: u64,
    ) -> impl Future<Output = Result<OutboxInventory, AnyError>> + Send;

    /// Puts up to `batch` of `scope`'s set-aside rows back in the queue, returning how many.
    ///
    /// A row another re-queue holds is skipped, never cleared twice. Call it until it returns zero.
    ///
    /// # Errors
    /// Returns [`AnyError`] if the store cannot be written; a chunk is re-queued whole or not at all.
    fn requeue(
        &self,
        scope: &R::Scope,
        batch: u64,
    ) -> impl Future<Output = Result<u64, AnyError>> + Send;
}
