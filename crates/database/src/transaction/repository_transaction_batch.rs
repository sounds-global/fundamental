//! Batch operations inside a transaction.

use super::RepositoryTransaction;
use crate::DatabaseResult;
use fundamental_core::model::{BoundTo, Database};

/// Transaction-scoped batch operations that do **not** address rows by id, for `M` on `D`.
///
/// The counterpart of [`RepositoryBatch`](crate::RepositoryBatch).
pub trait RepositoryTransactionBatch<M, D>: RepositoryTransaction<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Inserts multiple models in a batch within the transaction.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if an insert fails.
    fn insert_many(&mut self, models: &[M]) -> impl Future<Output = DatabaseResult<Vec<M>>> + Send;

    /// Updates multiple models in a batch within the transaction.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if an update fails.
    fn update_many(&mut self, models: &[M]) -> impl Future<Output = DatabaseResult<Vec<M>>> + Send;
}
