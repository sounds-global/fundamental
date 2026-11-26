//! The sync-overlay metadata carried by every mirror row.

/// The conflict-resolution overlay stored alongside a synced row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncMeta {
    /// Server-authoritative last-modified time, in milliseconds since the Unix epoch.
    pub updated_at: i64,
    /// Monotonic version counter.
    pub version: i64,
    /// Whether the row is a tombstone (a propagated delete).
    pub tombstone: bool,
}

impl SyncMeta {
    /// Creates metadata for a live (non-deleted) row.
    pub fn new(updated_at: i64, version: i64) -> Self {
        Self {
            updated_at,
            version,
            tombstone: false,
        }
    }

    /// Marks this metadata as a tombstone (a delete to propagate).
    #[must_use]
    pub fn tombstoned(mut self) -> Self {
        self.tombstone = true;
        self
    }
}
