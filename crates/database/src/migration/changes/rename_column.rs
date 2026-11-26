//! [`RenameColumnChange`]: renames a column within a table.

/// Rename column change.
#[derive(Debug, Clone, PartialEq)]
pub struct RenameColumnChange {
    /// Table name.
    pub table_name: String,
    /// Old column name.
    pub old_name: String,
    /// New column name.
    pub new_name: String,
}
