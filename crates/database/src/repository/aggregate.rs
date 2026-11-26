//! Aggregate operations for repositories.

use super::{DatabaseResult, QueryBuilder, Repository};
use fundamental_core::model::{BoundTo, Database, LeadingKeyAddressed, Satisfied};

/// Extension trait for aggregate operations.
pub trait RepositoryAggregate<M, D>: Repository<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Counts all models in the table.
    ///
    /// Use [`RepositoryAggregate::count_where`] to count only the models matching a query.
    fn count(&self) -> impl Future<Output = DatabaseResult<u64>> + Send;

    /// Counts models matching the current query.
    fn count_where<State>(
        &self,
        query: &QueryBuilder<M, D, State>,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send
    where
        LeadingKeyAddressed<M, D, State>: Satisfied;
}
