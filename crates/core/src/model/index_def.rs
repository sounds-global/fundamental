//! Schema-metadata description of a table index.

/// Index definition for schema metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDef {
    /// Index name.
    pub name: &'static str,
    /// Columns included in the index.
    pub columns: &'static [&'static str],
    /// Whether this is a unique index.
    pub unique: bool,
    /// Partial index condition (WHERE clause).
    pub condition: Option<&'static str>,
}

impl IndexDef {
    /// Creates a new index definition.
    pub const fn new(name: &'static str, columns: &'static [&'static str]) -> Self {
        Self {
            name,
            columns,
            unique: false,
            condition: None,
        }
    }

    /// Creates a unique index.
    pub const fn unique(mut self) -> Self {
        self.unique = true;
        self
    }

    /// Adds a partial index condition.
    pub const fn with_condition(mut self, condition: &'static str) -> Self {
        self.condition = Some(condition);
        self
    }
}
