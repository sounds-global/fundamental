//! [`Facts`]: the schema a migration set builds, as far as its text tells.

use std::collections::{BTreeMap, BTreeSet};

use crate::migration::ddl::{DdlColumn, DdlStatement, KeyDeclaration, Sharding};

/// A key's parts: each one's columns and how it is split.
pub(crate) type KeyParts = Vec<(Vec<String>, Option<Sharding>)>;

/// What the files read so far make: each table's columns, key and indexes.
#[derive(Debug, Clone, Default)]
pub(crate) struct Facts {
    /// The tables, by name.
    pub tables: BTreeMap<String, TableFacts>,
    /// The table each index is on.
    pub index_tables: BTreeMap<String, String>,
}

/// One table, as the files read so far make it.
#[derive(Debug, Clone, Default)]
pub(crate) struct TableFacts {
    /// The file that created it.
    pub created_in: String,
    /// Its columns, by name.
    pub columns: BTreeMap<String, DdlColumn>,
    /// Its key's parts: each one's columns and how it is split.
    pub key: KeyParts,
    /// Its indexes, each with its key's parts.
    pub indexes: BTreeMap<String, KeyParts>,
    /// What its `CREATE TABLE` says of colocation.
    pub colocated: Option<bool>,
}

impl TableFacts {
    fn created(migration: &str, statement: &DdlStatement) -> Option<Self> {
        let DdlStatement::CreateTable {
            primary_key,
            columns,
            colocated,
            ..
        } = statement
        else {
            return None;
        };
        Some(Self {
            created_in: migration.to_owned(),
            columns: columns
                .iter()
                .map(|column| (column.name.clone(), column.clone()))
                .collect(),
            key: primary_key.as_ref().map(parts).unwrap_or_default(),
            indexes: BTreeMap::new(),
            colocated: *colocated,
        })
    }

    /// The column `name`, when the files declared it.
    pub fn column(&self, name: &str) -> Option<&DdlColumn> {
        self.columns.get(name)
    }

    /// Every column some key or index leads with.
    pub fn leading_columns(&self) -> BTreeSet<&str> {
        std::iter::once(&self.key)
            .chain(self.indexes.values())
            .filter_map(|parts| parts.first())
            .filter_map(|(columns, _)| columns.first())
            .map(String::as_str)
            .collect()
    }
}

pub(crate) fn parts(key: &KeyDeclaration) -> KeyParts {
    key.elements()
        .iter()
        .map(|element| (element.columns().to_vec(), element.sharding()))
        .collect()
}

impl Facts {
    /// The table `statement` works on, when it names one.
    pub fn table_of(&self, statement: &DdlStatement) -> Option<String> {
        match statement {
            DdlStatement::CreateTable { table, .. }
            | DdlStatement::CreateIndex { table, .. }
            | DdlStatement::AddColumn { table, .. }
            | DdlStatement::AlterColumnType { table, .. }
            | DdlStatement::SetNotNull { table, .. }
            | DdlStatement::AddPrimaryKey { table, .. }
            | DdlStatement::Data { table, .. } => Some(table.clone()),
            DdlStatement::Destructive { verb, object } => Some(match object.split_once('.') {
                Some((table, _)) => table.to_owned(),
                None if verb == "DROP INDEX" => self
                    .index_tables
                    .get(object)
                    .cloned()
                    .unwrap_or_else(|| object.clone()),
                None => object.clone(),
            }),
            DdlStatement::CreateRole { .. } | DdlStatement::Other { .. } => None,
        }
    }

    pub fn apply(&mut self, migration: &str, statement: &DdlStatement) {
        match statement {
            DdlStatement::CreateTable { table, .. } => {
                if let Some(created) = TableFacts::created(migration, statement) {
                    self.tables.entry(table.clone()).or_insert(created);
                }
            }
            DdlStatement::CreateIndex {
                name, table, key, ..
            } => {
                self.index_tables.insert(name.clone(), table.clone());
                if let Some(facts) = self.tables.get_mut(table) {
                    facts.indexes.insert(name.clone(), parts(key));
                }
            }
            DdlStatement::AddColumn {
                table,
                column,
                data_type,
                default,
                ..
            } => {
                if let Some(facts) = self.tables.get_mut(table) {
                    facts.columns.insert(
                        column.clone(),
                        DdlColumn {
                            name: column.clone(),
                            data_type: data_type.clone(),
                            default: default.clone(),
                        },
                    );
                }
            }
            DdlStatement::AlterColumnType { table, column, to } => {
                if let Some(found) = self
                    .tables
                    .get_mut(table)
                    .and_then(|facts| facts.columns.get_mut(column))
                {
                    found.data_type.clone_from(to);
                }
            }
            DdlStatement::Destructive { verb, object } => self.remove(verb, object),
            DdlStatement::SetNotNull { .. }
            | DdlStatement::AddPrimaryKey { .. }
            | DdlStatement::Data { .. }
            | DdlStatement::CreateRole { .. }
            | DdlStatement::Other { .. } => {}
        }
    }

    fn remove(&mut self, verb: &str, object: &str) {
        match (verb, object.split_once('.')) {
            ("DROP TABLE", _) => {
                self.tables.remove(object);
            }
            ("DROP INDEX", _) => {
                if let Some(table) = self.index_tables.remove(object)
                    && let Some(facts) = self.tables.get_mut(&table)
                {
                    facts.indexes.remove(object);
                }
            }
            ("DROP COLUMN", Some((table, column))) => {
                if let Some(facts) = self.tables.get_mut(table) {
                    facts.columns.remove(column);
                }
            }
            _ => {}
        }
    }
}
