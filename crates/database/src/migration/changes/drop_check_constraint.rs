//! The [`DropCheckConstraintChange`] schema change.

/// Drop check constraint change.
#[derive(Debug, Clone, PartialEq)]
pub struct DropCheckConstraintChange {
    /// Table name.
    pub table_name: String,
    /// Constraint name.
    pub constraint_name: String,
    /// Its condition, as the database holds it.
    pub expression: String,
}
