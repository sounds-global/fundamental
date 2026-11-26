//! The [`DropForeignKeyChange`] schema change.

/// Drop foreign key change.
#[derive(Debug, Clone, PartialEq)]
pub struct DropForeignKeyChange {
    /// Table name.
    pub table_name: String,
    /// Constraint name.
    pub constraint_name: String,
}
