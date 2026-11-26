//! The [`ChangeWriterChange`] schema change.

/// A role the model names as a writer of a table (`written_by`) that holds no `INSERT`,
/// `UPDATE` or `DELETE` on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeWriterChange {
    /// Table name.
    pub table_name: String,
    /// The writing role.
    pub role: String,
}

impl ChangeWriterChange {
    /// What the change does, in words.
    #[must_use]
    pub fn description(&self) -> String {
        format!(
            "Let '{}' write the rows of '{}' (it writes none)",
            self.role, self.table_name
        )
    }
}
