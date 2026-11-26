use fundamental_sync::SyncRecord;

/// A single change a recompute handler makes to a projection table: rewrite a row, or
/// tombstone one whose source rows have gone.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProjectionUpdate {
    /// Write a recomputed projection row; its payload must carry the owner column.
    Upsert(SyncRecord),

    /// Tombstone the projection row `id`, scoped to `owner`.
    Tombstone {
        /// The projection row's primary key, rendered as text.
        id: String,
        /// The value of the projection's owner column for the deleted row.
        owner: String,
    },
}

impl ProjectionUpdate {
    /// Rewrites a projection row.
    #[must_use]
    pub fn upsert(record: SyncRecord) -> Self {
        Self::Upsert(record)
    }

    /// Tombstones the projection row `id`, owned by `owner`.
    #[must_use]
    pub fn tombstone(id: impl Into<String>, owner: impl Into<String>) -> Self {
        Self::Tombstone {
            id: id.into(),
            owner: owner.into(),
        }
    }
}
