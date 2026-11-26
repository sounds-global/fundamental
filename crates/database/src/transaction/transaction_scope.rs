//! [`TransactionScope`]: what may be done inside a transaction without ending it.

use crate::DatabaseResult;
use fundamental_core::model::Database;

/// The part of a transaction that works inside it: savepoints and, through
/// [`RepositoryTransaction`](crate::RepositoryTransaction), typed reads and writes.
///
/// Split from [`Transaction`](crate::Transaction) so a handle can be lent to code that must
/// not end the transaction.
pub trait TransactionScope<D>: Send + Sync
where
    D: Database,
{
    /// Creates a savepoint within the transaction.
    fn savepoint(&mut self, name: &str) -> impl Future<Output = DatabaseResult<()>> + Send;

    /// Rolls back to a savepoint.
    fn rollback_to_savepoint(
        &mut self,
        name: &str,
    ) -> impl Future<Output = DatabaseResult<()>> + Send;

    /// Releases a savepoint.
    fn release_savepoint(&mut self, name: &str) -> impl Future<Output = DatabaseResult<()>> + Send;
}
