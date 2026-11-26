//! Convenience operations inside a transaction.

use super::{KeyedRepositoryTransaction, RepositoryTransaction};
use crate::DatabaseResult;
use fundamental_core::model::{BoundTo, Database};

/// Transaction-scoped convenience operations for a model `M` on database `D`.
///
/// The counterpart of [`RepositoryExt`](crate::RepositoryExt).
pub trait RepositoryTransactionExt<M, D>: RepositoryTransaction<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Finds a model by its ID, or inserts the one produced by `create` if absent.
    ///
    /// Requires an addressable id; a composite-key model has no `find`.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if a query fails.
    fn find_or_create<F>(
        &mut self,
        id: &Self::Id,
        create: F,
    ) -> impl Future<Output = DatabaseResult<M>> + Send
    where
        F: FnOnce() -> M + Send,
        Self: KeyedRepositoryTransaction<M, D>,
    {
        async move {
            match self.find(id).await? {
                Some(found) => Ok(found),
                None => self.insert(&create()).await,
            }
        }
    }

    /// Updates a model only if it already exists, returning `None` otherwise.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if a query fails.
    fn update_if_exists(
        &mut self,
        model: &M,
    ) -> impl Future<Output = DatabaseResult<Option<M>>> + Send;
}
