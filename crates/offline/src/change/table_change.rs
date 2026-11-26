//! [`TableChange`] — a notification that one local table's row changed.

use crate::change::ChangeOp;
use crate::db::Value;

/// A local write: the table, the affected row's primary key and the kind of change.
///
/// Carries only the row's identity; a watcher re-reads through the store.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TableChange {
    /// The SQLite table that changed (a model's `SQLITE_TABLE` / `MIRROR_TABLE`).
    pub table: String,
    /// The primary key of the affected row.
    pub id: Value,
    /// What happened to the row.
    pub op: ChangeOp,
}

impl TableChange {
    /// Builds a change for `table`, row `id`, kind `op`.
    #[must_use]
    pub fn new(table: impl Into<String>, id: Value, op: ChangeOp) -> Self {
        Self {
            table: table.into(),
            id,
            op,
        }
    }
}
