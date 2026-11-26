//! Builder for SQL-wire queries that carry a row lock.

use crate::query::RowLock;
use fundamental_core::model::{BoundTo, NoLeadingKey, SqlDb};
use fundamental_db::QueryBuilder;

/// YSQL query builder with row-lock support.
#[derive(Debug, Clone)]
pub struct SqlQuery<M, State = NoLeadingKey>
where
    M: BoundTo<SqlDb>,
{
    inner: QueryBuilder<M, SqlDb, State>,
    row_lock: Option<RowLock>,
}

impl<M, State> SqlQuery<M, State>
where
    M: BoundTo<SqlDb>,
{
    /// Creates a new YSQL query from a QueryBuilder, keeping its leading-key state.
    pub fn new(builder: QueryBuilder<M, SqlDb, State>) -> Self {
        Self {
            inner: builder,
            row_lock: None,
        }
    }

    /// Locks the returned rows with `FOR UPDATE SKIP LOCKED`, so concurrent readers take disjoint rows.
    ///
    /// Crate-private: see [`RowLock`]. Downstream code claims through `OutboxClaimer`.
    #[must_use]
    pub(crate) fn for_update_skip_locked(mut self) -> Self {
        self.row_lock = Some(RowLock::ForUpdateSkipLocked);
        self
    }

    /// Locks the returned rows with `FOR UPDATE`, waiting for any another transaction holds.
    ///
    /// For one row chosen by a predicate, as the first statement of a transaction.
    #[must_use]
    pub fn for_update(mut self) -> Self {
        self.row_lock = Some(RowLock::ForUpdate);
        self
    }

    /// Returns the row-locking clause if set.
    #[must_use]
    pub(crate) fn get_row_lock(&self) -> Option<RowLock> {
        self.row_lock
    }

    /// Returns the inner QueryBuilder.
    pub fn into_inner(self) -> QueryBuilder<M, SqlDb, State> {
        self.inner
    }

    /// Returns a reference to the inner QueryBuilder.
    pub fn inner(&self) -> &QueryBuilder<M, SqlDb, State> {
        &self.inner
    }
}
