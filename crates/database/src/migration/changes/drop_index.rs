//! The [`DropIndexChange`] schema change.

/// Drop index change.
#[derive(Debug, Clone, PartialEq)]
pub struct DropIndexChange {
    /// Table name.
    pub table_name: String,
    /// Index name.
    pub index_name: String,
}
