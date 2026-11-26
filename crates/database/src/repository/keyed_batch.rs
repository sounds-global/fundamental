//! Batch operations that address rows by id.

use super::{DatabaseResult, Repository, RepositoryBatch};
use fundamental_core::model::{BoundTo, Database};

/// Extension trait for batch operations addressed by id.
///
/// A composite-key model cannot implement it, so a call that would bind half a key is a
/// compile error.
pub trait KeyedRepositoryBatch<M, D>: RepositoryBatch<M, D> + Repository<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Finds multiple models by their IDs.
    ///
    /// Returns models in arbitrary order; ids not found are skipped.
    fn find_many(&self, ids: &[Self::Id]) -> impl Future<Output = DatabaseResult<Vec<M>>> + Send;

    /// Deletes multiple models by their IDs.
    ///
    /// Returns the number of models deleted.
    fn delete_many(&self, ids: &[Self::Id]) -> impl Future<Output = DatabaseResult<u64>> + Send;
}
