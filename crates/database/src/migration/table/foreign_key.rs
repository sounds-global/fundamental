use super::ForeignKeyAction;

/// Foreign key definition for schema.
#[derive(Debug, Clone, PartialEq)]
pub struct TableForeignKey {
    /// Constraint name.
    pub name: String,
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
