//! The [`Syncable`] trait: a model that mirrors to a local offline database.

use fundamental_core::model::Model;
use fundamental_core::model::offline::ConflictStrategy;

use crate::SyncColumn;

/// A model that participates in offline sync, describing its offline mirror table.
pub trait Syncable: Model {
    /// The conflict-resolution strategy for this model.
    const STRATEGY: ConflictStrategy;

    /// The mirror table name in the offline database.
    const MIRROR_TABLE: &'static str;

    /// The mirror table's columns: the synced data columns, then the overlay columns.
    fn sync_columns() -> &'static [SyncColumn];

    /// The mirror table's primary-key column(s).
    fn primary_key() -> &'static [&'static str];
}
