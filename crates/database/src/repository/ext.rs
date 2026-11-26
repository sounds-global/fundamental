//! Convenience operations for repositories.

use super::{DatabaseResult, Repository};
use fundamental_core::model::{BoundTo, Database};

/// Convenience extension trait for repository operations.
pub trait RepositoryExt<M, D>: Repository<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Finds a model or creates it if not found.
    ///
    /// The `create` closure is only called if the model doesn't exist.
    fn find_or_create<F>(
        &self,
        id: &Self::Id,
        create: F,
    ) -> impl Future<Output = DatabaseResult<M>> + Send
    where
        F: FnOnce() -> M + Send;

    /// Updates a model if it exists.
    ///
    /// Returns `None` if the model doesn't exist.
    fn update_if_exists(&self, model: &M)
    -> impl Future<Output = DatabaseResult<Option<M>>> + Send;
}
