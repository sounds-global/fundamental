//! Building the offline mirror [`TableSchema`] from a [`Syncable`] model.

use fundamental_db::migration::{TableColumn, TableSchema};
use fundamental_offline::db::OfflineDatabase;

use crate::{SyncError, Syncable};

/// Builds the offline mirror table schema for a [`Syncable`] model.
pub fn mirror_table_schema<M: Syncable>() -> TableSchema {
    let mut schema = TableSchema::new(M::MIRROR_TABLE);
    let primary_key = M::primary_key();

    for column in M::sync_columns() {
        schema.add_column(TableColumn {
            name: column.name.to_string(),
            data_type: column.storage_type.to_string(),
            nullable: column.nullable,
            default: None,
            primary_key: primary_key.contains(&column.name),
            unique: false,
        });
    }

    schema.set_primary_key(primary_key.iter().map(|c| (*c).to_string()).collect());
    schema
}

/// Creates the offline mirror table for `M` if it does not already exist.
///
///
/// # Errors
/// Returns [`SyncError::Storage`] if the table cannot be created.
pub fn ensure_mirror_table<M: Syncable>(db: &OfflineDatabase) -> Result<(), SyncError> {
    let schema = mirror_table_schema::<M>();

    let mut definitions: Vec<String> = schema
        .columns
        .values()
        .map(|column: &TableColumn| {
            let nullable = if column.nullable { "" } else { " NOT NULL" };
            format!("{} {}{}", column.name, column.data_type, nullable)
        })
        .collect();
    // Columns live in a HashMap; sorted so the emitted DDL is deterministic.
    definitions.sort();

    let primary_key = if schema.primary_key.is_empty() {
        String::new()
    } else {
        format!(", PRIMARY KEY ({})", schema.primary_key.join(", "))
    };

    let sql = format!(
        "CREATE TABLE IF NOT EXISTS {} ({}{});",
        M::MIRROR_TABLE,
        definitions.join(", "),
        primary_key
    );
    db.execute_batch(&sql)
        .map_err(|error| SyncError::Storage(error.to_string()))?;
    Ok(())
}
