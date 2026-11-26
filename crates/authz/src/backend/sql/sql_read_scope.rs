//! [`SqlReadScope`]: one evaluation's transaction on the SQL wire.

use fundamental_sql::SqlTransaction;

use crate::tuple::{ReadScope, WallTime};
use crate::zookie::HybridTs;

/// A `REPEATABLE READ` transaction, held open for exactly one evaluation.
///
/// A live MVCC snapshot, so there is no staleness window. Behind a mutex because concurrent
/// branches share one session.
pub struct SqlReadScope {
    tx: futures::lock::Mutex<SqlTransaction>,
    ts: HybridTs,
}

impl SqlReadScope {
    pub(crate) fn new(tx: SqlTransaction, ts: HybridTs) -> Self {
        Self {
            tx: futures::lock::Mutex::new(tx),
            ts,
        }
    }

    pub(crate) fn tx(&self) -> &futures::lock::Mutex<SqlTransaction> {
        &self.tx
    }

    /// Consumes the scope for its transaction.
    ///
    /// By value, so the close cannot race a read.
    pub(crate) fn into_transaction(self) -> SqlTransaction {
        self.tx.into_inner()
    }
}

impl ReadScope for SqlReadScope {
    fn ts(&self) -> HybridTs {
        self.ts
    }

    /// The cluster clock the zookie is stamped with, so the two instants cannot differ.
    fn now(&self) -> WallTime {
        WallTime::from_unix_nanos(self.ts.wall)
    }
}
