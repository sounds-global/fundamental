//! [`SqlTransactionScope`]: one decision's reads, taken through a caller's transaction.

use fundamental_core::model::SqlDb;
use fundamental_db::QueryBuilder;
use fundamental_sql::SqlTransaction;
use futures::lock::Mutex;

use super::relation_tuple_row::RelationTupleRow;
use super::sql_tuple_store::SqlTupleStore;
use crate::error::AuthzError;
use crate::tuple::{ReadScope, RelationTuple, WallTime};
use crate::zookie::HybridTs;

/// A read scope over a transaction the caller owns and will commit.
///
/// Under `SERIALIZABLE` the database records what was read, so a conflicting write fails the
/// caller's commit. Its instant is [`HybridTs::ZERO`], since nothing has committed; its `now`
/// is the cluster's clock, since a grant judged at the epoch would admit forever.
pub(crate) struct SqlTransactionScope<'t> {
    tx: Mutex<&'t mut SqlTransaction>,
    now: WallTime,
}

impl<'t> SqlTransactionScope<'t> {
    /// A scope reading through `tx` until it is dropped, its caveats judged at `now`.
    pub(crate) fn new(tx: &'t mut SqlTransaction, now: WallTime) -> Self {
        Self {
            tx: Mutex::new(tx),
            now,
        }
    }

    pub(super) async fn query(
        &self,
        builder: QueryBuilder<RelationTupleRow, SqlDb>,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let mut tx = self.tx.lock().await;
        SqlTupleStore::query_in(&mut tx, builder).await
    }
}

impl ReadScope for SqlTransactionScope<'_> {
    fn ts(&self) -> HybridTs {
        HybridTs::ZERO
    }

    fn now(&self) -> WallTime {
        self.now
    }
}
