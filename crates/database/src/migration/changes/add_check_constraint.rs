//! The [`AddCheckConstraintChange`] schema change.

/// Add check constraint change.
#[derive(Debug, Clone, PartialEq)]
pub struct AddCheckConstraintChange {
    /// Table name.
    pub table_name: String,
    /// Constraint name.
    pub constraint_name: String,
    /// Check expression.
    pub expression: String,
}
