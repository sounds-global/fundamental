//! The full-database side of the diff model.

use std::collections::HashMap;

use crate::migration::{MigrationDatabase, TableSchema};

/// Represents the complete schema of a database.
#[derive(Debug, Clone, Default)]
pub struct DatabaseSchema {
    /// Tables in the database.
    pub tables: HashMap<String, TableSchema>,
    /// Database type.
    pub database_type: Option<MigrationDatabase>,
}

impl DatabaseSchema {
    /// Creates a new empty schema.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a schema for a specific database type.
    pub fn for_database(database: MigrationDatabase) -> Self {
        Self {
            tables: HashMap::new(),
            database_type: Some(database),
        }
    }

    /// Adds a table to the schema.
    pub fn add_table(&mut self, table: TableSchema) {
        self.tables.insert(table.name.clone(), table);
    }

    /// Gets a table by name.
    pub fn get_table(&self, name: &str) -> Option<&TableSchema> {
        self.tables.get(name)
    }
}
