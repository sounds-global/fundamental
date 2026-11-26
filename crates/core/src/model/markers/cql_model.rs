//! The [`CqlModel`] extension trait.

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

use super::{BoundTo, CqlDb};
use crate::Model;

/// Extension trait for models bound to YCQL.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a YCQL model",
    label = "missing YCQL model implementation",
    note = "Use `#[derive(Model)]` with `#[model(cql)]`.",
    note = "YCQL models must have at least one `#[model(partition_key)]` field."
)]
pub trait CqlModel: BoundTo<CqlDb> + Model {
    /// The table name for this model; the keyspace comes from the connection it is routed to.
    const TABLE: &'static str;

    /// The domain this model belongs to (the named connection / routing key).
    const DOMAIN: &'static str = "default";

    /// Whether this model is soft-deletable (`#[model(soft_delete)]`); default reads then exclude deleted rows.
    const SOFT_DELETE: bool = false;

    /// The framework-managed soft-delete timestamp column name.
    const DELETED_AT_COLUMN: &'static str = "deleted_at";

    /// The partition key columns' values, a tuple in partition-key order (the bare type for one column).
    ///
    /// A partition therefore cannot be addressed by half of its key.
    type PartitionKey: Send + Sync;

    /// The clustering key columns (if any).
    type ClusteringKey: Send + Sync;

    /// Columns holding a [`Localized`](crate::i18n::Localized) field's source-locale value.
    fn translatable_columns() -> &'static [&'static str] {
        &[]
    }

    /// The companion translations table (`<table>_i18n`), if any field is translatable.
    fn translation_table() -> Option<&'static str> {
        None
    }

    /// The companion-table rows for this model's translatable fields.
    fn translation_rows(&self) -> Vec<crate::i18n::TranslationRow> {
        Vec::new()
    }

    /// Puts one companion-table row back on its translatable field; a no-op for any other column.
    fn apply_translation(&mut self, row: crate::i18n::TranslationRow) {
        let _ = row;
    }
}

/// A YCQL model whose partition key is one column: the bound on `CqlQuery::partition_key`.
///
/// A composite-partition model does not implement it, so addressing it by one value is a compile error.
pub trait SinglePartitionModel: CqlModel {}

/// A YCQL model whose partition key is more than one column.
pub trait CompositePartitionModel: CqlModel {
    /// Returns this row's partition key values, for addressing its partition again.
    fn partition_key_values(&self) -> Self::PartitionKey;
}
