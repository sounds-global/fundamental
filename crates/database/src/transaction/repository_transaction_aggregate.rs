//! Aggregate operations inside a transaction.

use super::RepositoryTransaction;
use crate::{DatabaseResult, QueryBuilder};
use fundamental_core::model::{BoundTo, Database, LeadingKeyAddressed, Satisfied};

/// Transaction-scoped aggregate operations for a model `M` on database `D`.
///
/// The counterpart of [`RepositoryAggregate`](crate::RepositoryAggregate).
pub trait RepositoryTransactionAggregate<M, D>: RepositoryTransaction<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Counts all models in the table within the transaction.
    ///
    /// Use [`count_where`](Self::count_where) to count only matching models.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails.
    fn count(&mut self) -> impl Future<Output = DatabaseResult<u64>> + Send;

    /// Counts models matching `query` within the transaction.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails.
    fn count_where<State>(
        &mut self,
        query: &QueryBuilder<M, D, State>,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send
    where
        LeadingKeyAddressed<M, D, State>: Satisfied;
}
