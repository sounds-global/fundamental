//! One column of an offline mirror table.

use crate::SyncColumnRole;

/// A single column in a model's offline mirror table: its name, SQLite storage
/// type, nullability, and [role](SyncColumnRole).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncColumn {
    /// Column name in the mirror table.
    pub name: &'static str,
    /// SQLite storage type (e.g. `TEXT`, `INTEGER`).
    pub storage_type: &'static str,
    /// Whether the column is nullable.
    pub nullable: bool,
    /// What the column is for.
    pub role: SyncColumnRole,
}
