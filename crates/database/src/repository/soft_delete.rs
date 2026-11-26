//! Soft-delete operations for repositories.

use super::{DatabaseResult, Repository};
use fundamental_core::model::{BoundTo, Database};

/// Extension trait for soft-delete operations.
///
/// Soft delete marks records as deleted without removing them.
pub trait RepositorySoftDelete<M, D>: Repository<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Soft-deletes a model by its ID.
    ///
    /// Sets the `deleted_at` timestamp without removing the record.
    fn soft_delete(&self, id: &Self::Id) -> impl Future<Output = DatabaseResult<bool>> + Send;

    /// Restores a soft-deleted model.
    fn restore(&self, id: &Self::Id) -> impl Future<Output = DatabaseResult<bool>> + Send;

    /// Finds a model including soft-deleted ones.
    fn find_with_deleted(
        &self,
        id: &Self::Id,
    ) -> impl Future<Output = DatabaseResult<Option<M>>> + Send;

    /// Permanently deletes a soft-deleted model.
    fn purge(&self, id: &Self::Id) -> impl Future<Output = DatabaseResult<bool>> + Send;
}
