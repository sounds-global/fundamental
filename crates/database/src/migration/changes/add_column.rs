//! The [`AddColumnChange`] schema change.

use crate::migration::TableColumn;

/// Add column change.
#[derive(Debug, Clone, PartialEq)]
pub struct AddColumnChange {
    /// Table name.
    pub table_name: String,
    /// Column definition.
    pub column: TableColumn,
}
