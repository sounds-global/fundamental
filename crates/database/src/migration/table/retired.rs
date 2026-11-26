/// What a model no longer holds: the columns, indexes and checks the next migration drops.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Retired {
    /// Columns to drop.
    pub columns: Vec<String>,
    /// Indexes to drop.
    pub indexes: Vec<String>,
    /// Checks to drop.
    pub checks: Vec<String>,
}

impl Retired {
    /// Whether nothing is retired.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty() && self.indexes.is_empty() && self.checks.is_empty()
    }
}
