//! Options controlling how [`SchemaDiffer`](super::SchemaDiffer) compares schemas.

/// Configuration for schema diffing.
#[derive(Debug, Clone, Default)]
pub struct DiffConfig {
    /// Whether to ignore column order changes.
    pub ignore_column_order: bool,
    /// Whether to ignore index name differences.
    pub ignore_index_names: bool,
    /// Whether to detect renamed columns (by type similarity).
    pub detect_renames: bool,
}
