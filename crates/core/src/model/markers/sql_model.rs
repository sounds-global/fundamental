//! The [`SqlModel`] extension trait.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::{BoundTo, SqlDb};
use crate::Model;
use crate::model::IndexDef;

/// Extension trait for models bound to YugabyteDB's YSQL API.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a YugabyteDB YSQL model",
    label = "missing YSQL model implementation",
    note = "Use `#[derive(Model)]` with `#[model(sql)]`.",
    note = "If this model is for YCQL (`#[model(cql)]`), use the CQL repository instead."
)]
pub trait SqlModel: BoundTo<SqlDb> + Model {
    /// The type of this model's primary key: a tuple in declaration order when there is more than one `#[model(id)]`.
    ///
    /// A composite key's `Id` is not `sqlx::Encode`, so `find_by_id` on it is a compile error rather than a prefix-less scan.
    type Id;

    /// Returns a reference to this model's primary key value.
    fn id(&self) -> &Self::Id;

    /// The table name for this model.
    const TABLE: &'static str;

    /// The primary key column name; the first key column when the key is composite.
    const PRIMARY_KEY: &'static str = "id";

    /// Every primary-key column, in key order.
    const KEY_COLUMNS: &'static [&'static str] = &[Self::PRIMARY_KEY];

    /// The secondary indexes this model declares with `#[model(index(...))]`, in declaration order.
    const INDEXES: &'static [IndexDef] = &[];
    /// The domain this model belongs to (for multi-domain database routing).
    const DOMAIN: &'static str = "default";

    /// Whether this model is soft-deletable (`#[model(soft_delete)]`); default reads then exclude deleted rows.
    const SOFT_DELETE: bool = false;

    /// The framework-managed soft-delete timestamp column name.
    const DELETED_AT_COLUMN: &'static str = "deleted_at";

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

    /// This model as its row reads back: moments cut to microseconds, fields the table does not keep at their default.
    #[must_use]
    fn into_stored(self) -> Self
    where
        Self: Sized,
    {
        self
    }
}

/// A YSQL model whose primary key is one column: the bound on every fetch-by-id path.
///
/// A composite-key model does not implement it, so a lookup by part of its key is a compile error, not a scan.
pub trait SingleKeyModel: SqlModel {}

/// A YSQL model whose primary key is more than one column, with the key's values as a typed tuple.
pub trait CompositeKeyModel: SqlModel {
    /// The key columns' values, as a tuple in key order.
    type KeyValues;

    /// Returns this row's key values, for addressing it again.
    fn key_values(&self) -> Self::KeyValues;
}
