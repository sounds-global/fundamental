//! [`RenameTableChange`]: renames a table.

/// Rename table change.
#[derive(Debug, Clone, PartialEq)]
pub struct RenameTableChange {
    /// Old table name.
    pub old_name: String,
    /// New table name.
    pub new_name: String,
}
