//! Addressing a row by its id, inside a transaction.
//!
//! Split from [`RepositoryTransaction`] so a composite-key model, whose `Id` is a marker that
//! cannot be bound, can still be written inside a transaction.

use super::RepositoryTransaction;
use crate::DatabaseResult;
use fundamental_core::model::{BoundTo, Database};

/// Transaction-scoped operations that address a row by its id.
///
/// Implemented only for models with exactly one key column, so `tx.find(id)` on a composite-key
/// model is a compile error.
pub trait KeyedRepositoryTransaction<M, D>: RepositoryTransaction<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Finds a model by its ID within the transaction.
    ///
    /// Returns `None` if no row matches.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails.
    fn find(&mut self, id: &Self::Id) -> impl Future<Output = DatabaseResult<Option<M>>> + Send;

    /// Finds a model by its ID within the transaction, erroring if not found.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails or no
    /// row matches.
    fn find_required(&mut self, id: &Self::Id) -> impl Future<Output = DatabaseResult<M>> + Send;

    /// Checks whether a model with the given ID exists within the transaction.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails.
    fn exists(&mut self, id: &Self::Id) -> impl Future<Output = DatabaseResult<bool>> + Send;

    /// Deletes a model by its ID within the transaction.
    ///
    /// Returns `true` if a row was deleted, `false` if none matched.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the delete fails.
    fn delete(&mut self, id: &Self::Id) -> impl Future<Output = DatabaseResult<bool>> + Send;
}
