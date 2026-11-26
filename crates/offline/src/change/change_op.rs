//! [`ChangeOp`] — the kind of local write a [`TableChange`](super::TableChange) reports.

/// How a row changed in a local table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChangeOp {
    /// A row was inserted.
    Insert,
    /// An existing row was updated (or upserted).
    Update,
    /// A row was removed (a hard delete or a soft delete that hides it).
    Delete,
}
