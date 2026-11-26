//! Batch operations for repositories.
//!
//! Split from [`KeyedRepositoryBatch`](super::KeyedRepositoryBatch) for the same reason as
//! [`Repository`] and [`KeyedRepository`](super::KeyedRepository): a composite-key model has no
//! bindable id, and must still be batch-written.

use super::{DatabaseResult, Repository};
use fundamental_core::model::{BoundTo, Database};

/// Extension trait for batch operations that do **not** address rows by id.
///
/// A row's own key columns address it, so these work whatever the key's width.
pub trait RepositoryBatch<M, D>: Repository<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Inserts multiple models in a batch.
    fn insert_many(&self, models: &[M]) -> impl Future<Output = DatabaseResult<Vec<M>>> + Send;

    /// Updates multiple models in a batch.
    fn update_many(&self, models: &[M]) -> impl Future<Output = DatabaseResult<Vec<M>>> + Send;
}
