//! The [`AlterColumnChange`] schema change.

use crate::migration::TableColumn;

/// Alter column change.
#[derive(Debug, Clone, PartialEq)]
pub struct AlterColumnChange {
    /// Table name.
    pub table_name: String,
    /// Column name.
    pub column_name: String,
    /// Old column definition.
    pub old_column: TableColumn,
    /// New column definition.
    pub new_column: TableColumn,
}
