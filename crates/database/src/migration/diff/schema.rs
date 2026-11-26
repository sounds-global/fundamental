//! The core diff engine that turns two [`DatabaseSchema`]s into [`SchemaChange`]s.

use std::collections::{HashMap, HashSet, VecDeque};

use super::{DatabaseSchema, DiffConfig};
use crate::migration::table::condition_words;
use crate::migration::{
    AddCheckConstraintChange, AddColumnChange, AddForeignKeyChange, AlterColumnChange,
    ChangeKeyChange, ChangeKeyStructureChange, ChangePlacementChange, ChangeReaderChange,
    ChangeWriterChange, CreateIndexChange, CreateTableChange, DropCheckConstraintChange,
    DropColumnChange, DropForeignKeyChange, DropIndexChange, DropTableChange, MigrationDatabase,
    RenameColumnChange, ReorderColumnsChange, SchemaChange, TableColumn, TableIndex, TableSchema,
};

/// Schema diff engine.
#[derive(Debug)]
pub struct SchemaDiffer {
    config: DiffConfig,
}

impl SchemaDiffer {
    /// Creates a new differ with default config.
    pub fn new() -> Self {
        Self {
            config: DiffConfig::default(),
        }
    }

    /// Creates a differ with custom config.
    pub fn with_config(config: DiffConfig) -> Self {
        Self { config }
    }

    /// Computes the diff between expected schema (from models) and actual schema (from database).
    ///
    /// Returns the changes needed to transform `actual` into `expected`.
    #[expect(clippy::missing_panics_doc, reason = "readability baseline")]
    pub fn diff(&self, expected: &DatabaseSchema, actual: &DatabaseSchema) -> Vec<SchemaChange> {
        let mut changes = Vec::new();

        let expected_tables: HashSet<_> = expected.tables.keys().collect();
        let actual_tables: HashSet<_> = actual.tables.keys().collect();

        for table_name in expected_tables.difference(&actual_tables) {
            let table = expected.tables.get(*table_name).unwrap();
            changes.push(self.create_table_change(table));
        }

        for table_name in actual_tables.difference(&expected_tables) {
            changes.push(SchemaChange::DropTable(DropTableChange {
                table_name: (*table_name).clone(),
                cascade: false,
            }));
        }

        // Only the SQL reader reads back a key; a CQL key is its partition and clustering key.
        let keys_read = !matches!(
            actual.database_type,
            Some(MigrationDatabase::OfflineDb | MigrationDatabase::CqlDb)
        );
        for table_name in expected_tables.intersection(&actual_tables) {
            let expected_table = expected.tables.get(*table_name).unwrap();
            let actual_table = actual.tables.get(*table_name).unwrap();
            changes.extend(self.diff_table(expected_table, actual_table, keys_read));
        }

        changes
    }

    fn create_table_change(&self, table: &TableSchema) -> SchemaChange {
        SchemaChange::CreateTable(CreateTableChange {
            table_name: table.name.clone(),
            columns: table
                .column_order
                .iter()
                .filter_map(|name| table.columns.get(name).cloned())
                .collect(),
            primary_key: table.primary_key.clone(),
            indexes: table.indexes.values().cloned().collect(),
            foreign_keys: table.foreign_keys.values().cloned().collect(),
            check_constraints: table.check_constraints.values().cloned().collect(),
            partition_key: table.partition_key.clone(),
            clustering_key: table.clustering_key.clone(),
            table_options: table.table_options.clone(),
        })
    }

    fn diff_table(
        &self,
        expected: &TableSchema,
        actual: &TableSchema,
        keys_read: bool,
    ) -> Vec<SchemaChange> {
        let mut changes = Vec::new();

        if let (Some(exp_pk), Some(act_pk)) = (&expected.partition_key, &actual.partition_key)
            && (exp_pk != act_pk || expected.clustering_key != actual.clustering_key)
        {
            changes.push(SchemaChange::ChangeKeyStructure(ChangeKeyStructureChange {
                table_name: expected.name.clone(),
                old_partition_key: act_pk.clone(),
                new_partition_key: exp_pk.clone(),
                old_clustering_key: actual.clustering_key.clone().unwrap_or_default(),
                new_clustering_key: expected.clustering_key.clone().unwrap_or_default(),
            }));
            return changes;
        }

        // A table built without a key is keyed on a hidden row id.
        if keys_read && !expected.key.is_empty() && expected.key != actual.key {
            changes.push(SchemaChange::ChangeKey(ChangeKeyChange {
                table_name: expected.name.clone(),
                old_key: actual.key.clone(),
                new_key: expected.key.clone(),
            }));
        }

        changes.extend(self.diff_columns(expected, actual));

        changes.extend(self.diff_indexes(expected, actual));

        changes.extend(self.diff_foreign_keys(expected, actual));

        changes.extend(self.diff_check_constraints(expected, actual));

        if keys_read {
            changes.extend(Self::diff_readers(expected, actual));
            changes.extend(Self::diff_writers(expected, actual));
            changes.extend(Self::diff_placement(expected, actual));
        }

        changes
    }

    /// Each role the model lets write a table that the table does not; extra table writers are
    /// no drift, since a deployment's default privileges grant its login every table.
    fn diff_writers(expected: &TableSchema, actual: &TableSchema) -> Vec<SchemaChange> {
        expected
            .writers
            .difference(&actual.writers)
            .map(|role| {
                SchemaChange::ChangeWriter(ChangeWriterChange {
                    table_name: expected.name.clone(),
                    role: role.clone(),
                })
            })
            .collect()
    }

    /// The table placed otherwise than its model says. More tablets than declared are no drift,
    /// since the engine splits a growing table on its own.
    fn diff_placement(expected: &TableSchema, actual: &TableSchema) -> Vec<SchemaChange> {
        let colocated = match (actual.colocated, expected.colocated) {
            (Some(is), Some(declared)) if is != declared => Some((is, declared)),
            _ => None,
        };
        let tablets = expected
            .tablets
            .filter(|declared| actual.tablets.is_some_and(|has| has < *declared))
            .map(|declared| (actual.tablets, declared));
        if colocated.is_none() && tablets.is_none() {
            return Vec::new();
        }
        vec![SchemaChange::ChangePlacement(ChangePlacementChange {
            table_name: expected.name.clone(),
            colocated,
            tablets,
        })]
    }

    /// Each reader whose rows differ between the model and the table, compared as the engine means
    /// them.
    fn diff_readers(expected: &TableSchema, actual: &TableSchema) -> Vec<SchemaChange> {
        let roles: std::collections::BTreeSet<&String> = expected
            .readers
            .keys()
            .chain(actual.readers.keys())
            .collect();
        roles
            .into_iter()
            .filter_map(|role| {
                let new = expected.readers.get(role);
                let old = actual.readers.get(role);
                let same =
                    new.map(|when| condition_words(when)) == old.map(|when| condition_words(when));
                (!same).then(|| {
                    SchemaChange::ChangeReader(ChangeReaderChange {
                        table_name: expected.name.clone(),
                        role: role.clone(),
                        old: old.cloned(),
                        new: new.cloned(),
                    })
                })
            })
            .collect()
    }

    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn diff_columns(&self, expected: &TableSchema, actual: &TableSchema) -> Vec<SchemaChange> {
        let mut changes = Vec::new();

        let expected_cols: HashSet<_> = expected.columns.keys().collect();
        let actual_cols: HashSet<_> = actual.columns.keys().collect();

        let added_cols: Vec<_> = expected_cols.difference(&actual_cols).collect();
        let dropped_cols: Vec<_> = actual_cols.difference(&expected_cols).collect();

        let mut renamed_added: HashSet<&str> = HashSet::new();
        let mut renamed_dropped: HashSet<&str> = HashSet::new();

        // Renames: bucket the added columns by normalized (type, nullable) once, then pop one per
        // dropped column, O(D + A); popping from the front keeps the first unmatched add winning.
        if self.config.detect_renames {
            let mut added_by_signature: HashMap<(String, bool), VecDeque<&str>> = HashMap::new();
            for added_name in &added_cols {
                let added_col = expected.columns.get(**added_name).unwrap();
                let signature = (
                    self.normalize_type(&added_col.data_type),
                    added_col.nullable,
                );
                added_by_signature
                    .entry(signature)
                    .or_default()
                    .push_back(added_name.as_str());
            }

            for dropped_name in &dropped_cols {
                let dropped_col = actual.columns.get(**dropped_name).unwrap();
                let signature = (
                    self.normalize_type(&dropped_col.data_type),
                    dropped_col.nullable,
                );

                if let Some(candidate) = added_by_signature
                    .get_mut(&signature)
                    .and_then(VecDeque::pop_front)
                {
                    changes.push(SchemaChange::RenameColumn(RenameColumnChange {
                        table_name: expected.name.clone(),
                        old_name: (**dropped_name).clone(),
                        new_name: candidate.to_string(),
                    }));
                    renamed_added.insert(candidate);
                    renamed_dropped.insert(dropped_name);
                }
            }
        }

        for col_name in added_cols {
            if renamed_added.contains(col_name.as_str()) {
                continue;
            }
            let column = expected.columns.get(*col_name).unwrap();
            changes.push(SchemaChange::AddColumn(AddColumnChange {
                table_name: expected.name.clone(),
                column: column.clone(),
            }));
        }

        for col_name in dropped_cols {
            if renamed_dropped.contains(col_name.as_str()) {
                continue;
            }
            changes.push(SchemaChange::DropColumn(DropColumnChange {
                table_name: expected.name.clone(),
                column_name: (*col_name).clone(),
            }));
        }

        for col_name in expected_cols.intersection(&actual_cols) {
            let expected_col = expected.columns.get(*col_name).unwrap();
            let actual_col = actual.columns.get(*col_name).unwrap();

            if self.columns_differ(expected_col, actual_col) {
                changes.push(SchemaChange::AlterColumn(AlterColumnChange {
                    table_name: expected.name.clone(),
                    column_name: (*col_name).clone(),
                    old_column: actual_col.clone(),
                    new_column: expected_col.clone(),
                }));
            }
        }

        if !self.config.ignore_column_order && expected.column_order != actual.column_order {
            let expected_set: HashSet<_> = expected.column_order.iter().collect();
            let actual_set: HashSet<_> = actual.column_order.iter().collect();
            if expected_set == actual_set {
                changes.push(SchemaChange::ReorderColumns(ReorderColumnsChange {
                    table_name: expected.name.clone(),
                    new_order: expected.column_order.clone(),
                }));
            }
        }

        changes
    }

    fn columns_differ(&self, expected: &TableColumn, actual: &TableColumn) -> bool {
        if self.normalize_type(&expected.data_type) != self.normalize_type(&actual.data_type) {
            return true;
        }

        if expected.nullable != actual.nullable {
            return true;
        }

        // A default as the engine means it: `''` is `''::text`.
        if expected
            .default
            .as_deref()
            .map(crate::migration::condition_words)
            != actual
                .default
                .as_deref()
                .map(crate::migration::condition_words)
        {
            return true;
        }

        false
    }

    pub(crate) fn normalize_type(&self, sql_type: &str) -> String {
        let t = sql_type.to_uppercase();
        match t.as_str() {
            "INT" | "INT4" | "INTEGER" => "INTEGER".to_string(),
            "BIGINT" | "INT8" => "BIGINT".to_string(),
            "SMALLINT" | "INT2" => "SMALLINT".to_string(),
            "BOOL" | "BOOLEAN" => "BOOLEAN".to_string(),
            "VARCHAR" => "TEXT".to_string(),
            _ => {
                if t.starts_with("VARCHAR(") || t.starts_with("CHAR(") {
                    "TEXT".to_string()
                } else {
                    t
                }
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn diff_indexes(&self, expected: &TableSchema, actual: &TableSchema) -> Vec<SchemaChange> {
        let mut changes = Vec::new();

        if self.config.ignore_index_names {
            let mut matched_actual: HashSet<&str> = HashSet::new();

            for expected_idx in expected.indexes.values() {
                let matching_actual = actual.indexes.values().find(|actual_idx| {
                    !matched_actual.contains(actual_idx.name.as_str())
                        && self.indexes_structurally_equal(expected_idx, actual_idx)
                });

                if let Some(actual_idx) = matching_actual {
                    matched_actual.insert(&actual_idx.name);
                } else {
                    changes.push(SchemaChange::CreateIndex(CreateIndexChange {
                        table_name: expected.name.clone(),
                        index_name: expected_idx.name.clone(),
                        columns: expected_idx.columns.clone(),
                        unique: expected_idx.unique,
                        condition: expected_idx.condition.clone(),
                    }));
                }
            }

            for actual_idx in actual.indexes.values() {
                if !matched_actual.contains(actual_idx.name.as_str()) {
                    changes.push(SchemaChange::DropIndex(DropIndexChange {
                        table_name: expected.name.clone(),
                        index_name: actual_idx.name.clone(),
                    }));
                }
            }
        } else {
            let expected_indexes: HashSet<_> = expected.indexes.keys().collect();
            let actual_indexes: HashSet<_> = actual.indexes.keys().collect();

            for idx_name in expected_indexes.difference(&actual_indexes) {
                let index = expected.indexes.get(*idx_name).unwrap();
                changes.push(SchemaChange::CreateIndex(CreateIndexChange {
                    table_name: expected.name.clone(),
                    index_name: index.name.clone(),
                    columns: index.columns.clone(),
                    unique: index.unique,
                    condition: index.condition.clone(),
                }));
            }

            for idx_name in actual_indexes.difference(&expected_indexes) {
                changes.push(SchemaChange::DropIndex(DropIndexChange {
                    table_name: expected.name.clone(),
                    index_name: (*idx_name).clone(),
                }));
            }

            for idx_name in expected_indexes.intersection(&actual_indexes) {
                let expected_idx = expected.indexes.get(*idx_name).unwrap();
                let actual_idx = actual.indexes.get(*idx_name).unwrap();

                if !expected_idx.same_as(actual_idx) {
                    changes.push(SchemaChange::DropIndex(DropIndexChange {
                        table_name: expected.name.clone(),
                        index_name: (*idx_name).clone(),
                    }));
                    changes.push(SchemaChange::CreateIndex(CreateIndexChange {
                        table_name: expected.name.clone(),
                        index_name: expected_idx.name.clone(),
                        columns: expected_idx.columns.clone(),
                        unique: expected_idx.unique,
                        condition: expected_idx.condition.clone(),
                    }));
                }
            }
        }

        changes
    }

    fn indexes_structurally_equal(&self, a: &TableIndex, b: &TableIndex) -> bool {
        a.same_as(b)
    }

    fn diff_foreign_keys(&self, expected: &TableSchema, actual: &TableSchema) -> Vec<SchemaChange> {
        let mut changes = Vec::new();

        let expected_fks: HashSet<_> = expected.foreign_keys.keys().collect();
        let actual_fks: HashSet<_> = actual.foreign_keys.keys().collect();

        for fk_name in expected_fks.difference(&actual_fks) {
            let fk = expected.foreign_keys.get(*fk_name).unwrap();
            changes.push(SchemaChange::AddForeignKey(AddForeignKeyChange {
                table_name: expected.name.clone(),
                constraint_name: fk.name.clone(),
                columns: fk.columns.clone(),
                references_table: fk.references_table.clone(),
                references_columns: fk.references_columns.clone(),
                on_delete: fk.on_delete,
                on_update: fk.on_update,
            }));
        }

        for fk_name in actual_fks.difference(&expected_fks) {
            changes.push(SchemaChange::DropForeignKey(DropForeignKeyChange {
                table_name: expected.name.clone(),
                constraint_name: (*fk_name).clone(),
            }));
        }

        changes
    }

    fn diff_check_constraints(
        &self,
        expected: &TableSchema,
        actual: &TableSchema,
    ) -> Vec<SchemaChange> {
        let mut changes = Vec::new();

        let expected_checks: HashSet<_> = expected.check_constraints.keys().collect();
        let actual_checks: HashSet<_> = actual.check_constraints.keys().collect();

        for check_name in expected_checks.difference(&actual_checks) {
            let check = expected.check_constraints.get(*check_name).unwrap();
            changes.push(SchemaChange::AddCheckConstraint(AddCheckConstraintChange {
                table_name: expected.name.clone(),
                constraint_name: check.name.clone(),
                expression: check.expression.clone(),
            }));
        }

        // A check is compared by name: the engine rewrites a condition's text.

        for check_name in actual_checks.difference(&expected_checks) {
            changes.push(SchemaChange::DropCheckConstraint(
                DropCheckConstraintChange {
                    table_name: expected.name.clone(),
                    constraint_name: (*check_name).clone(),
                    expression: actual.check_constraints[*check_name].expression.clone(),
                },
            ));
        }

        changes
    }
}

impl Default for SchemaDiffer {
    fn default() -> Self {
        Self::new()
    }
}
