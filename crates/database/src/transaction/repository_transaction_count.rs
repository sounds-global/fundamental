//! A count over a window of time inside a transaction.

use chrono::{DateTime, Utc};

use super::RepositoryTransaction;
use crate::{DatabaseResult, Filter};
use fundamental_core::model::{BoundTo, Database};

/// How many rows of `M` have their time column in a window, across the whole table.
///
/// Returns a number, never a row, so it needs no leading key. To see which rows matched,
/// use [`count_where`](super::RepositoryTransactionAggregate::count_where).
pub trait RepositoryTransactionCount<M, D>: RepositoryTransaction<M, D>
where
    M: BoundTo<D> + Send + Sync,
    D: Database,
{
    /// Counts the rows whose `at` lies in `[from, until)` and that every one of `codes`
    /// matches.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the query fails or a column is unknown.
    fn count_between(
        &mut self,
        at: &'static str,
        from: DateTime<Utc>,
        until: DateTime<Utc>,
        codes: Vec<Filter>,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send;
}
