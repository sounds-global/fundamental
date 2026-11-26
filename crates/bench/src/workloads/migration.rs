//! A schema compared with what a database holds, and the next migration generated.

use fundamental_core::model::sql::ShardKind;
use fundamental_db::migration::generate::{Snapshot, next_statements};
use fundamental_db::migration::{
    DatabaseSchema, KeyPart, Lints, SchemaDiffer, TableColumn, TableIndex, TableSchema,
};

/// How many tables the schema holds: an app of some size.
pub const TABLES: usize = 50;

const COLUMNS: usize = 8;

/// A [`TABLES`]-table schema, a database a migration behind it, and their snapshots.
#[derive(Debug, Clone)]
pub struct Migration {
    expected: DatabaseSchema,
    actual: DatabaseSchema,
    before: Snapshot,
    models: Snapshot,
    lints: Lints,
}

impl Migration {
    /// The models' schema, and the database's, each table lacking the newest column and index.
    #[must_use]
    pub fn new() -> Self {
        let models: Vec<TableSchema> = (0..TABLES).map(|n| table(n, true)).collect();
        let written: Vec<TableSchema> = (0..TABLES).map(|n| table(n, false)).collect();
        let mut expected = DatabaseSchema::new();
        let mut actual = DatabaseSchema::new();
        for table in &models {
            expected.add_table(table.clone());
        }
        for table in &written {
            actual.add_table(table.clone());
        }
        Self {
            expected,
            actual,
            before: Snapshot::of(&written, None),
            models: Snapshot::of(&models, None),
            lints: Lints::default(),
        }
    }

    /// The differences the differ finds between the models and the database; how many.
    #[must_use]
    pub fn diff(&self) -> usize {
        SchemaDiffer::new().diff(&self.expected, &self.actual).len()
    }

    /// The statements of the next migration; how many, or none when one is refused.
    #[must_use]
    pub fn generate(&self) -> Option<usize> {
        next_statements(&self.before, &self.models, &self.lints)
            .ok()
            .map(|statements| statements.len())
    }

    /// The differences between the snapshot the migrations produce and the models'; how many.
    #[must_use]
    pub fn differences(&self) -> usize {
        self.models.differences(&self.before).len()
    }
}

impl Default for Migration {
    fn default() -> Self {
        Self::new()
    }
}

fn table(n: usize, newest: bool) -> TableSchema {
    let name = format!("table_{n:02}");
    let mut table = TableSchema::new(name.clone());
    table.add_column(column("organisation_id", "UUID", false));
    table.add_column(column("id", "UUID", false));
    for c in 0..COLUMNS {
        table.add_column(column(&format!("field_{c}"), "TEXT", true));
    }
    table.set_key(KeyPart::parts(&[
        ("organisation_id", Some(ShardKind::Hash)),
        ("id", Some(ShardKind::Asc)),
    ]));
    if newest {
        table.add_column(column("archived_at", "TIMESTAMPTZ", true));
        table.add_index(TableIndex {
            name: format!("{name}_by_archived"),
            columns: Vec::new(),
            unique: false,
            condition: None,
            key: KeyPart::parts(&[
                ("organisation_id", Some(ShardKind::Hash)),
                ("archived_at", Some(ShardKind::Asc)),
            ]),
            include: Vec::new(),
            constraint: false,
        });
    }
    table
}

fn column(name: &str, data_type: &str, nullable: bool) -> TableColumn {
    TableColumn {
        name: name.to_owned(),
        data_type: data_type.to_owned(),
        nullable,
        default: None,
        primary_key: false,
        unique: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_has_a_difference() {
        let migration = Migration::new();
        assert!(migration.diff() >= TABLES);
        assert!(migration.differences() >= TABLES);
    }

    #[test]
    fn the_next_migration_adds_each_column_and_index() {
        assert_eq!(Migration::new().generate(), Some(2 * TABLES));
    }
}
