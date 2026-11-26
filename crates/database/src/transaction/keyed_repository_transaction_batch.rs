//! Transaction-scoped batch operations that address rows by id; a composite-key model has no
//! bindable id.

use super::{RepositoryTransaction, RepositoryTransactionBatch};
use crate::DatabaseResult;
use fundamental_core::model::{BoundTo, Database};

/// Transaction-scoped batch operations addressed by id, for `M` on `D`.
pub trait KeyedRepositoryTransactionBatch<M, D>:
    RepositoryTransactionBatch<M, D> + RepositoryTransaction<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Finds multiple models by their IDs within the transaction.
    ///
    /// Returns models in arbitrary order; ids not found are skipped.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if a query fails.
    fn find_many(
        &mut self,
        ids: &[Self::Id],
    ) -> impl Future<Output = DatabaseResult<Vec<M>>> + Send;

    /// Deletes multiple models by their IDs within the transaction.
    ///
    /// Returns the number of rows deleted.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if a delete fails.
    fn delete_many(&mut self, ids: &[Self::Id])
    -> impl Future<Output = DatabaseResult<u64>> + Send;
}
