//! [`ReorderColumnsChange`]: changes the column order of a table.

/// Reorder columns change.
#[derive(Debug, Clone, PartialEq)]
pub struct ReorderColumnsChange {
    /// Table name.
    pub table_name: String,
    /// New column order.
    pub new_order: Vec<String>,
}
