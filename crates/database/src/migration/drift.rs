//! Schema drift reporting.

/// Schema drift between model and database.
#[derive(Debug, Clone)]
pub struct SchemaDrift {
    /// Table name.
    pub table_name: String,
    /// Description of the drift.
    pub description: String,
    /// Expected (from model).
    pub expected: String,
    /// Actual (from database).
    pub actual: String,
}
