//! A table schema as the SQLite a device runs for it.

use crate::migration::TableSchema;

/// `CREATE TABLE` for an SQLite `table`: its columns and its key's columns.
#[must_use]
pub fn create_sqlite_table(table: &TableSchema) -> String {
    let mut lines: Vec<String> = table
        .column_order
        .iter()
        .filter_map(|name| table.columns.get(name))
        .map(|column| {
            let mut line = format!("    {} {}", column.name, column.data_type);
            if !column.nullable {
                line.push_str(" NOT NULL");
            }
            if let Some(default) = &column.default {
                line.push_str(" DEFAULT ");
                line.push_str(default);
            }
            line
        })
        .collect();
    if !table.primary_key.is_empty() {
        lines.push(format!(
            "    PRIMARY KEY ({})",
            table.primary_key.join(", ")
        ));
    }
    format!(
        "CREATE TABLE IF NOT EXISTS {} (\n{}\n)",
        table.name,
        lines.join(",\n")
    )
}

#[cfg(test)]
mod tests {
    use super::create_sqlite_table;
    use crate::migration::{MigrationDatabase, TableColumn, TableSchema};

    #[test]
    fn a_companion_carries_its_records_key_column_and_type() {
        let mut drafts = TableSchema::new("drafts");
        drafts.add_column(TableColumn {
            name: "id".to_owned(),
            data_type: "INTEGER".to_owned(),
            nullable: false,
            default: None,
            primary_key: true,
            unique: false,
        });
        drafts.set_primary_key(vec!["id".to_owned()]);

        assert_eq!(
            create_sqlite_table(&drafts.translations(MigrationDatabase::OfflineDb)),
            "CREATE TABLE IF NOT EXISTS drafts_i18n (\n    id INTEGER NOT NULL,\n    \
             field TEXT NOT NULL,\n    locale TEXT NOT NULL,\n    value TEXT NOT NULL,\n    \
             source_hash TEXT NOT NULL DEFAULT '',\n    \
             translation_hash TEXT NOT NULL DEFAULT '',\n    checked_by TEXT,\n    \
             checked_at INTEGER,\n    PRIMARY KEY (id, field, locale)\n)"
        );
    }
}
