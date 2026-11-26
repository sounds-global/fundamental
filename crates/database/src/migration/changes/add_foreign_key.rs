//! The [`AddForeignKeyChange`] schema change.

use crate::migration::ForeignKeyAction;

/// Add foreign key change.
#[derive(Debug, Clone, PartialEq)]
pub struct AddForeignKeyChange {
    /// Table name.
    pub table_name: String,
    /// Constraint name.
    pub constraint_name: String,
    /// Local columns.
    pub columns: Vec<String>,
    /// Referenced table.
    pub references_table: String,
    /// Referenced columns.
    pub references_columns: Vec<String>,
    /// On delete action.
    pub on_delete: ForeignKeyAction,
    /// On update action.
    pub on_update: ForeignKeyAction,
}
