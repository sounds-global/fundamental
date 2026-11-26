//! The [`QueryExecutor`] trait.
//!
//! Every method requires a [`LeadingKeyAddressed`] witness, so a query that cannot address a
//! row never reaches a database.

use super::{FilterValue, QueryBuilder};
use crate::DatabaseResult;
use fundamental_core::model::{BoundTo, Database, LeadingKeyAddressed, Satisfied};

/// Trait for executing queries.
///
/// Implemented by each backend driver.
pub trait QueryExecutor<M, D>: Send + Sync
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Executes the query and returns all matching models.
    fn execute<State>(
        &self,
        query: &QueryBuilder<M, D, State>,
    ) -> impl Future<Output = DatabaseResult<Vec<M>>> + Send
    where
        LeadingKeyAddressed<M, D, State>: Satisfied;

    /// Executes the query and returns the first matching model.
    fn execute_one<State>(
        &self,
        query: &QueryBuilder<M, D, State>,
    ) -> impl Future<Output = DatabaseResult<Option<M>>> + Send
    where
        LeadingKeyAddressed<M, D, State>: Satisfied;

    /// Executes the query and returns the count of matching models.
    fn count<State>(
        &self,
        query: &QueryBuilder<M, D, State>,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send
    where
        LeadingKeyAddressed<M, D, State>: Satisfied;

    /// Executes a delete based on the query filters.
    fn delete_where<State>(
        &self,
        query: &QueryBuilder<M, D, State>,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send
    where
        LeadingKeyAddressed<M, D, State>: Satisfied;

    /// Executes an update based on the query filters.
    fn update_where<State>(
        &self,
        query: &QueryBuilder<M, D, State>,
        updates: &[(String, FilterValue)],
    ) -> impl Future<Output = DatabaseResult<u64>> + Send
    where
        LeadingKeyAddressed<M, D, State>: Satisfied;
}
