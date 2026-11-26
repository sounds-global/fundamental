//! Deriving a table's expected schema from a model definition, per backend; the impls are
//! emitted by `#[derive(Model)]`.

use fundamental_core::model::Database;

use crate::migration::{MigrationDatabase, TableSchema};

/// A model that can describe its own expected table schema for backend `D`.
///
///
/// The returned [`TableSchema`] mirrors the `CREATE TABLE` the derive emits.
pub trait ModelSchema<D: Database> {
    /// The migration target this schema describes.
    const MIGRATION_DATABASE: MigrationDatabase;

    /// Builds the table schema this model expects to find in backend `D`.
    fn table_schema() -> TableSchema;

    /// The companion table holding this model's `Localized` text in its other languages
    /// ([`TableSchema::translations`]), or `None` when it has none.
    fn translation_schema() -> Option<TableSchema> {
        None
    }
}
