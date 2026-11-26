//! Filtered read, update and delete inside a transaction.

use super::RepositoryTransaction;
use crate::{DatabaseResult, FilterValue, QueryBuilder};
use fundamental_core::model::{BoundTo, Database, LeadingKeyAddressed, Satisfied};

/// Transaction-scoped filtered query operations for a model `M` on database `D`.
///
/// The counterpart of [`QueryExecutor`](crate::QueryExecutor)'s filtered operations.
pub trait RepositoryTransactionQuery<M, D>: RepositoryTransaction<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Returns all models matching `query` within the transaction.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails.
    fn query_where<State>(
        &mut self,
        query: &QueryBuilder<M, D, State>,
    ) -> impl Future<Output = DatabaseResult<Vec<M>>> + Send
    where
        LeadingKeyAddressed<M, D, State>: Satisfied;

    /// Deletes all rows matching `query` within the transaction, returning the
    /// number of rows deleted; a `query` with no filters deletes every row.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the delete fails.
    fn delete_where<State>(
        &mut self,
        query: &QueryBuilder<M, D, State>,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send
    where
        LeadingKeyAddressed<M, D, State>: Satisfied;

    /// Updates all rows matching `query` within the transaction, setting each
    /// `(column, value)` in `updates`, and returns the number of rows updated; a `query` with no
    /// filters updates every row.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the update fails.
    fn update_where<State>(
        &mut self,
        query: &QueryBuilder<M, D, State>,
        updates: &[(String, FilterValue)],
    ) -> impl Future<Output = DatabaseResult<u64>> + Send
    where
        LeadingKeyAddressed<M, D, State>: Satisfied;
}
