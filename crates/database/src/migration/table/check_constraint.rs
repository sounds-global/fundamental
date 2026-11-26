/// Check constraint definition for schema.
#[derive(Debug, Clone, PartialEq)]
pub struct TableCheckConstraint {
    /// Constraint name.
    pub name: String,
    /// Check expression.
    pub expression: String,
}
