//! [`OutboxPump`]: the [`OutboxDrain`] over one outbox table.

use std::marker::PhantomData;
use std::pin::Pin;

use chrono::Utc;
use fundamental_core::AnyError;
use fundamental_core::model::Database;
use fundamental_core::outbox::{DrainReport, OutboxInventory};
use fundamental_db::{OutboxClaimer, OutboxRow, OutboxSetAside, Relay, RetryPolicy, drain_pending};

use crate::OutboxDrain;

/// Bounds the inventory cost; past it a count is reported as a floor.
const INVENTORY_CAP: u64 = 10_000;

/// Drains one outbox table, ticked by an [`OutboxSurface`](crate::OutboxSurface); its scope is fixed at construction.
pub struct OutboxPump<D, R, Claimer, Rel>
where
    D: Database,
    R: OutboxRow<D>,
{
    claimer: Claimer,
    relay: Rel,
    scope: R::Scope,
    policy: RetryPolicy,
    _row: PhantomData<fn() -> (D, R)>,
}

impl<D, R, Claimer, Rel> OutboxPump<D, R, Claimer, Rel>
where
    D: Database,
    R: OutboxRow<D>,
{
    /// Builds a pump over one outbox table, with the default retry policy.
    pub fn new(claimer: Claimer, relay: Rel, scope: R::Scope) -> Self {
        Self {
            claimer,
            relay,
            scope,
            policy: RetryPolicy::default(),
            _row: PhantomData,
        }
    }

    /// Uses a specific lease and backoff rather than the default.
    #[must_use]
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.policy = policy;
        self
    }
}

impl<D, R, Claimer, Rel> OutboxPump<D, R, Claimer, Rel>
where
    D: Database,
    R: OutboxRow<D>,
    Rel: Relay<R>,
{
    /// The policy every drain and inventory runs under, with the relay's own [`recurrence`](Relay::recurrence).
    #[must_use]
    pub fn retry_policy(&self) -> RetryPolicy {
        match self.relay.recurrence() {
            Some(period) => self.policy.with_recurrence(period),
            None => self.policy,
        }
    }
}

impl<D, R, Claimer, Rel> OutboxDrain for OutboxPump<D, R, Claimer, Rel>
where
    D: Database + Send + Sync + 'static,
    R: OutboxRow<D> + 'static,
    R::Scope: 'static,
    Claimer: OutboxClaimer<R, D> + OutboxSetAside<R, D> + 'static,
    Rel: Relay<R> + Send + Sync + 'static,
{
    fn drain(
        &self,
        batch: u64,
    ) -> Pin<Box<dyn Future<Output = Result<DrainReport, AnyError>> + Send + '_>> {
        Box::pin(drain_pending(
            &self.claimer,
            &self.relay,
            &self.scope,
            batch,
            self.retry_policy(),
            // Read per tick: a lease and backoff are relative to when the batch was taken.
            Utc::now(),
        ))
    }

    fn inventory(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<OutboxInventory, AnyError>> + Send + '_>> {
        Box::pin(self.claimer.inventory(
            &self.scope,
            Utc::now(),
            self.retry_policy(),
            INVENTORY_CAP,
        ))
    }
}
