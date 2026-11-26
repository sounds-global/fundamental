//! The role a column plays in an offline mirror table.

/// What a mirror-table column is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncColumnRole {
    /// A column copied from the model (a synced field).
    Data,
    /// The server-authoritative last-modified timestamp.
    UpdatedAt,
    /// A monotonic version counter.
    Version,
    /// A soft-delete marker.
    Tombstone,
}
