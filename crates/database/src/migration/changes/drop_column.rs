//! The [`DropColumnChange`] schema change.

/// Drop column change.
#[derive(Debug, Clone, PartialEq)]
pub struct DropColumnChange {
    /// Table name.
    pub table_name: String,
    /// Column name.
    pub column_name: String,
}
