//! Soft-delete operations inside a transaction.

use super::RepositoryTransaction;
use crate::DatabaseResult;
use fundamental_core::model::{BoundTo, Database};

/// Transaction-scoped soft-delete operations for a model `M` on database `D`.
///
/// The counterpart of [`RepositorySoftDelete`](crate::RepositorySoftDelete).
pub trait RepositoryTransactionSoftDelete<M, D>: RepositoryTransaction<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Soft-deletes a model by its ID, stamping `deleted_at`.
    ///
    /// Returns `true` if this call performed the soft delete.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails.
    fn soft_delete(&mut self, id: &Self::Id) -> impl Future<Output = DatabaseResult<bool>> + Send;

    /// Restores a soft-deleted model, clearing `deleted_at`.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails.
    fn restore(&mut self, id: &Self::Id) -> impl Future<Output = DatabaseResult<bool>> + Send;

    /// Finds a model by its ID including soft-deleted rows.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails.
    fn find_with_deleted(
        &mut self,
        id: &Self::Id,
    ) -> impl Future<Output = DatabaseResult<Option<M>>> + Send;

    /// Permanently deletes a model by its ID (hard delete).
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails.
    fn purge(&mut self, id: &Self::Id) -> impl Future<Output = DatabaseResult<bool>> + Send;
}
