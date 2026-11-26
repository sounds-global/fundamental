//! [`DropTableChange`]: drops a table, optionally cascading to dependent objects.

/// Drop table change.
#[derive(Debug, Clone, PartialEq)]
pub struct DropTableChange {
    /// Table name.
    pub table_name: String,
    /// Whether to cascade (drop dependent objects).
    pub cascade: bool,
}
