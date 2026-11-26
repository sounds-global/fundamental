//! Transaction-begin abstraction.

use super::{DatabaseResult, IsolationLevel, Transaction, TransactionOptions};
use fundamental_core::model::Database;

/// Trait for types that can begin transactions.
///
/// There is no default isolation level: a bare `BEGIN` inherits the server's, which differs
/// between engines and deployments.
pub trait Transactional<D>: Send + Sync
where
    D: Database,
{
    /// The transaction type.
    type Transaction: Transaction<D>;

    /// Begins a new transaction with `options`.
    ///
    /// The one required form; [`begin`](Self::begin) is expressed in terms of it.
    fn begin_with(
        &self,
        options: TransactionOptions,
    ) -> impl Future<Output = DatabaseResult<Self::Transaction>> + Send;

    /// Begins a new read-write transaction at `level`.
    fn begin(
        &self,
        level: IsolationLevel,
    ) -> impl Future<Output = DatabaseResult<Self::Transaction>> + Send {
        self.begin_with(TransactionOptions::new(level))
    }
}
