//! The [`CreateIndexChange`] schema change.

/// Create index change.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateIndexChange {
    /// Table name.
    pub table_name: String,
    /// Index name.
    pub index_name: String,
    /// Index columns.
    pub columns: Vec<String>,
    /// Whether this is a unique index.
    pub unique: bool,
    /// Partial index condition.
    pub condition: Option<String>,
}
