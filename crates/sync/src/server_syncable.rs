//! The [`ServerSyncable`] trait: a server model whose table carries the overlay.

/// A server model whose authoritative table carries the sync-overlay columns.
pub trait ServerSyncable {
    /// The authoritative server table name.
    const TABLE: &'static str;

    /// The columns that address a row, in key order.
    ///
    /// Deliberately without a default: a wrong default would match `UPDATE`/`DELETE` on a leading column only.
    const KEY_COLUMNS: &'static [&'static str];

    /// The leading key columns that form the storage partition, in key order; a prefix of
    /// [`KEY_COLUMNS`](Self::KEY_COLUMNS).
    const PARTITION_COLUMNS: &'static [&'static str];

    /// Whether this table accepts client pushes (`#[model(bidirectional)]`).
    const BIDIRECTIONAL: bool;

    /// The per-principal scoping column (`#[model(owner)]`); `None` for a global table.
    const OWNER_COLUMN: Option<&'static str> = None;

    /// The overlay column carrying the server-authoritative last-modified time.
    const UPDATED_AT_COLUMN: &'static str = "updated_at";

    /// The overlay column carrying the monotonic version counter.
    const VERSION_COLUMN: &'static str = "version";

    /// The overlay column marking a propagated delete.
    const TOMBSTONE_COLUMN: &'static str = "_tombstone";

    /// The synced data columns: every persisted field except the id, `no_sync` fields and secrets.
    fn data_columns() -> &'static [&'static str];
}
