//! [`OutboxEnqueue`]: enqueue an outbox row inside the writing transaction.

use std::future::Future;

use fundamental_core::model::Database;

use super::OutboxRow;
use crate::DatabaseResult;
use crate::transaction::RepositoryTransaction;

/// Enqueues an outbox row in the same transaction as the state change it records.
///
/// Call it inside the transaction beside the write it belongs to, so the row is durable if
/// and only if the change is. A [`drain_pending`](super::drain_pending) worker relays it
/// later; this is [`insert`](RepositoryTransaction::insert) under an intention-revealing name.
pub trait OutboxEnqueue<R, D>: RepositoryTransaction<R, D>
where
    R: OutboxRow<D>,
    D: Database,
{
    /// Inserts `row` as a pending outbox entry within this transaction.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the insert fails.
    fn enqueue(&mut self, row: &R) -> impl Future<Output = DatabaseResult<R>> + Send {
        self.insert(row)
    }
}

impl<T, R, D> OutboxEnqueue<R, D> for T
where
    T: RepositoryTransaction<R, D>,
    R: OutboxRow<D>,
    D: Database,
{
}
