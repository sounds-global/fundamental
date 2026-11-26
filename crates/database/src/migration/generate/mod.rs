//! The next migration, generated from the models: the difference between the schema the
//! migrations already written produce (their [`Snapshot`]) and the schema the models declare,
//! as one forward-only file, read without a database. [`rebase`] rewrites a branch's files on
//! top of main's ([`Main`]).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use fundamental_core::model::sql::{Level, Lint, ShardKind};

use crate::migration::render::{
    add_reader, add_writer, create_all, create_index, drop_reader, drop_writer, guard_rows,
    reader_policy, unguard_rows,
};
use crate::migration::{KeyPart, Lints, Retired, TableColumn, TableIndex, TableSchema};

/// The schema an owner's migrations produce, written beside them by the generator, with the
/// hash of the last file so two branches adding a file on the same one conflict.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The content hash of the migration this snapshot is the schema after; none before the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The tables, by name.
    #[serde(default)]
    pub tables: BTreeMap<String, SnapshotTable>,
    /// The tables the owner retired, for the next migration to drop; never written to the file.
    #[serde(skip)]
    pub retired: BTreeSet<String>,
}

/// A table in a [`Snapshot`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotTable {
    /// Its columns, in order.
    pub columns: Vec<SnapshotColumn>,
    /// Its key's parts.
    pub key: Vec<KeyPart>,
    /// Its indexes, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub indexes: BTreeMap<String, SnapshotIndex>,
    /// Its checks, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub checks: BTreeMap<String, String>,
    /// The roles that read it, each with the rows it sees.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub readers: BTreeMap<String, String>,
    /// The roles that write it.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub writers: BTreeSet<String>,
    /// Whether it shares the colocated database's tablet; none is as the database places it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colocated: Option<bool>,
    /// The tablets it is split into when created; none leaves it to the engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tablets: Option<u32>,
    /// The lints the model accepts on it, each with its reason; never written to the file.
    #[serde(skip)]
    pub accepted: BTreeMap<Lint, String>,
    /// The columns the model renamed, each to the name before; never written to the file.
    #[serde(skip)]
    pub renamed: BTreeMap<String, String>,
    /// What the model retired: never written to the file.
    #[serde(skip)]
    pub retired: Retired,
}

/// A column in a [`Snapshot`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotColumn {
    /// Its name.
    pub name: String,
    /// Its YSQL type.
    #[serde(rename = "type")]
    pub data_type: String,
    /// Whether it may be empty.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nullable: bool,
    /// Its default, as SQL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// An index in a [`Snapshot`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotIndex {
    /// Its key's parts.
    pub key: Vec<KeyPart>,
    /// Whether no two rows may hold the same key.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unique: bool,
    /// The rows it holds, when partial.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// The columns it carries beside its key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// Whether a table's `CONSTRAINT … UNIQUE` made it, so it is dropped as that constraint.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub constraint: bool,
}

impl Snapshot {
    /// The snapshot of `tables`, as the models declare them, after the migration `parent`.
    #[must_use]
    pub fn of(tables: &[TableSchema], parent: Option<String>) -> Self {
        Self {
            parent,
            tables: tables
                .iter()
                .map(|table| (table.name.clone(), SnapshotTable::of(table)))
                .collect(),
            retired: BTreeSet::new(),
        }
    }

    /// It with `tables` retired by their owner: the next migration drops each one still there.
    #[must_use]
    pub fn retiring(mut self, tables: &[&str]) -> Self {
        self.retired
            .extend(tables.iter().map(|table| (*table).to_owned()));
        self
    }

    /// It as its file holds it: canonical JSON, keys sorted, one value per line.
    ///
    /// # Panics
    /// Never: a snapshot holds only strings, flags and lists.
    #[must_use]
    pub fn to_file(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("a snapshot serialises");
        text.push('\n');
        text
    }

    /// The snapshot a file holds.
    ///
    /// # Errors
    /// The file is not a snapshot.
    pub fn from_file(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

impl SnapshotTable {
    fn of(table: &TableSchema) -> Self {
        Self {
            columns: table
                .column_order
                .iter()
                .filter_map(|name| table.columns.get(name))
                .map(|column| SnapshotColumn {
                    name: column.name.clone(),
                    data_type: column.data_type.clone(),
                    nullable: column.nullable,
                    default: column.default.clone(),
                })
                .collect(),
            key: table.key.clone(),
            indexes: table
                .indexes
                .values()
                .map(|index| {
                    (
                        index.name.clone(),
                        SnapshotIndex {
                            key: index.key.clone(),
                            unique: index.unique,
                            condition: index.condition.clone(),
                            include: index.include.clone(),
                            constraint: index.constraint,
                        },
                    )
                })
                .collect(),
            checks: table
                .check_constraints
                .values()
                .map(|check| (check.name.clone(), check.expression.clone()))
                .collect(),
            readers: table.readers.clone(),
            writers: table.writers.clone(),
            colocated: table.colocated,
            tablets: table.tablets,
            accepted: table.accepted.clone(),
            renamed: table
                .renamed
                .iter()
                .map(|(name, was)| (name.clone(), was.clone()))
                .collect(),
            retired: table.retired.clone(),
        }
    }

    /// The renames that take `was` to this table, as `(before, after)`.
    fn renames_from(&self, was: &Self) -> Vec<(String, String)> {
        self.renamed
            .iter()
            .filter(|(name, old)| was.column(old).is_some() && was.column(name).is_none())
            .map(|(name, old)| (old.clone(), name.clone()))
            .collect()
    }

    fn column(&self, name: &str) -> Option<&SnapshotColumn> {
        self.columns.iter().find(|column| column.name == name)
    }

    /// It after `renames`, every reference to a renamed column renamed.
    fn after(&self, renames: &[(String, String)]) -> Self {
        let name = |column: &String| {
            renames
                .iter()
                .find(|(old, _)| old == column)
                .map_or_else(|| column.clone(), |(_, new)| new.clone())
        };
        let parts = |key: &[KeyPart]| -> Vec<KeyPart> {
            key.iter()
                .map(|part| KeyPart {
                    columns: part.columns.iter().map(name).collect(),
                    ..part.clone()
                })
                .collect()
        };
        Self {
            columns: self
                .columns
                .iter()
                .map(|column| SnapshotColumn {
                    name: name(&column.name),
                    ..column.clone()
                })
                .collect(),
            key: parts(&self.key),
            indexes: self
                .indexes
                .iter()
                .map(|(index_name, index)| {
                    (
                        index_name.clone(),
                        SnapshotIndex {
                            key: parts(&index.key),
                            unique: index.unique,
                            condition: index
                                .condition
                                .as_deref()
                                .map(|condition| renamed_in(condition, renames)),
                            include: index.include.iter().map(name).collect(),
                            constraint: index.constraint,
                        },
                    )
                })
                .collect(),
            checks: self
                .checks
                .iter()
                .map(|(check, expression)| (check.clone(), renamed_in(expression, renames)))
                .collect(),
            readers: self
                .readers
                .iter()
                .map(|(role, when)| (role.clone(), renamed_in(when, renames)))
                .collect(),
            writers: self.writers.clone(),
            colocated: self.colocated,
            tablets: self.tablets,
            accepted: BTreeMap::new(),
            renamed: BTreeMap::new(),
            retired: Retired::default(),
        }
    }

    fn schema(&self, name: &str) -> TableSchema {
        let mut table = TableSchema::new(name);
        for column in &self.columns {
            table.add_column(TableColumn {
                name: column.name.clone(),
                data_type: column.data_type.clone(),
                nullable: column.nullable,
                default: column.default.clone(),
                primary_key: false,
                unique: false,
            });
        }
        table.set_key(self.key.clone());
        for (index_name, index) in &self.indexes {
            table.add_index(index.schema(index_name));
        }
        for (check_name, expression) in &self.checks {
            table.add_check_constraint(crate::migration::TableCheckConstraint {
                name: check_name.clone(),
                expression: expression.clone(),
            });
        }
        for (role, when) in &self.readers {
            table.add_reader(role.clone(), when.clone());
        }
        table.writers.clone_from(&self.writers);
        table.colocated = self.colocated;
        table.tablets = self.tablets;
        table
    }
}

impl SnapshotIndex {
    fn schema(&self, name: &str) -> TableIndex {
        TableIndex {
            name: name.to_owned(),
            columns: self
                .key
                .iter()
                .flat_map(|part| part.columns.clone())
                .collect(),
            unique: self.unique,
            condition: self.condition.clone(),
            key: self.key.clone(),
            include: self.include.clone(),
            constraint: self.constraint,
        }
    }
}

/// A change the generator will not write, because the models do not say what it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndeclaredChange {
    /// The table it is in.
    pub table: String,
    /// What would change, and what the model must declare for it to be written.
    pub reason: String,
}

/// The statements that take a database from `before` to the schema `models` declare: renames
/// and additions first, then what the models retired, tables last. Empty when the models
/// match their migrations.
///
/// `lints` are the project's, which decide what passes in a colocated database.
///
/// # Errors
/// Every change the models do not declare the meaning of, as an [`UndeclaredChange`].
pub fn next_statements(
    before: &Snapshot,
    models: &Snapshot,
    lints: &Lints,
) -> Result<Vec<String>, Vec<UndeclaredChange>> {
    let mut changes = Changes {
        lints: lints.clone(),
        ..Changes::default()
    };
    for (name, table) in &models.tables {
        if models.retired.contains(name) {
            changes.refuse(
                name,
                "the table is both declared and retired; keep one".to_owned(),
            );
        }
        match before.tables.get(name) {
            None => {
                changes.no_new_constraints(name, table.indexes.iter());
                let mut created = table.schema(name);
                if !lints.is_colocated() {
                    created.colocated = None;
                } else if created.colocated != Some(false)
                    && table.key.iter().any(|part| part.split == ShardKind::Hash)
                {
                    changes.refuse(
                        name,
                        "the table is colocated, and a colocated table's key cannot be hashed \
                         (the engine refuses it: `cannot colocate hash partitioned table`); split \
                         its key ASC or DESC, or give it its own tablets with \
                         `#[model(colocated = false)]`"
                            .to_owned(),
                    );
                }
                changes.statements.extend(create_all(&created));
            }
            Some(was) => changes.table(name, was, table),
        }
    }
    for old in before.tables.keys() {
        if models.tables.contains_key(old) {
            continue;
        }
        if models.retired.contains(old) {
            changes
                .dropped_tables
                .push(format!("DROP TABLE IF EXISTS {old};"));
        } else {
            changes.refuse(
                old,
                "the table is gone; list it in its owner's `retired = [...]` to drop it".to_owned(),
            );
        }
    }
    if changes.undeclared.is_empty() {
        Ok(changes.into_statements())
    } else {
        Err(changes.undeclared)
    }
}

#[derive(Default)]
struct Changes {
    lints: Lints,
    statements: Vec<String>,
    dropped_checks: Vec<String>,
    dropped_indexes: Vec<String>,
    dropped_columns: Vec<String>,
    dropped_tables: Vec<String>,
    undeclared: Vec<UndeclaredChange>,
}

impl Changes {
    fn refuse(&mut self, table: &str, reason: String) {
        self.undeclared.push(UndeclaredChange {
            table: table.to_owned(),
            reason,
        });
    }

    fn into_statements(self) -> Vec<String> {
        let mut statements = self.statements;
        statements.extend(self.dropped_checks);
        statements.extend(self.dropped_indexes);
        statements.extend(self.dropped_columns);
        statements.extend(self.dropped_tables);
        statements
    }

    fn table(&mut self, name: &str, was: &SnapshotTable, table: &SnapshotTable) {
        let mut by_old: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (new, old) in &table.renamed {
            by_old.entry(old).or_default().push(new);
        }
        for (old, news) in by_old.iter().filter(|(_, news)| news.len() > 1) {
            let news: Vec<String> = news.iter().map(|new| format!("`{new}`")).collect();
            self.refuse(
                name,
                format!(
                    "columns {} both say they `was` `{old}`; only one column can have been it",
                    news.join(" and ")
                ),
            );
        }
        let renames = table.renames_from(was);
        for (old, new) in &renames {
            self.statements
                .push(format!("ALTER TABLE {name} RENAME COLUMN {old} TO {new};"));
        }
        let was = &was.after(&renames);
        if was.key != table.key {
            self.refuse(
                name,
                format!(
                    "{}: its key changed, which rewrites the table; {}",
                    said(Lint::KeySplitChanged),
                    Lint::KeySplitChanged.fix()
                ),
            );
        }
        if was.colocated != table.colocated || was.tablets != table.tablets {
            self.refuse(
                name,
                format!(
                    "{}: its colocation or tablets changed, which the engine cannot change on a \
                     table that exists; create a new table and move to it",
                    said(Lint::TableRewrite)
                ),
            );
        }
        self.columns(name, was, table, &renames);
        self.indexes(name, was, table);
        self.checks(name, was, table);
        self.readers(name, was, table);
        self.writers(name, was, table);
    }

    /// The writer roles of a table both sides hold, granted or revoked.
    fn writers(&mut self, name: &str, was: &SnapshotTable, table: &SnapshotTable) {
        for role in table.writers.difference(&was.writers) {
            self.statements.extend(add_writer(name, role));
        }
        for role in was.writers.difference(&table.writers) {
            self.statements.extend(drop_writer(name, role));
        }
    }

    /// The reader roles of a table both sides hold, granted, changed or revoked.
    fn readers(&mut self, name: &str, was: &SnapshotTable, table: &SnapshotTable) {
        if was.readers.is_empty() {
            self.statements
                .extend(guard_rows(name, !table.readers.is_empty()));
        }
        for (role, when) in &table.readers {
            match was.readers.get(role) {
                None => self.statements.extend(add_reader(name, role, when)),
                Some(old) if old != when => self.statements.push(format!(
                    "ALTER POLICY {} ON {name} USING ({when});",
                    reader_policy(name, role)
                )),
                Some(_) => {}
            }
        }
        for role in was.readers.keys() {
            if !table.readers.contains_key(role) {
                self.statements.extend(drop_reader(name, role));
            }
        }
        if !was.readers.is_empty() && table.readers.is_empty() {
            self.statements.extend(unguard_rows(name));
        }
    }

    fn columns(
        &mut self,
        name: &str,
        was: &SnapshotTable,
        table: &SnapshotTable,
        renames: &[(String, String)],
    ) {
        for column in &table.columns {
            if table.retired.columns.contains(&column.name) {
                self.refuse(
                    name,
                    format!(
                        "column `{}` is both declared and retired; keep one",
                        column.name
                    ),
                );
            }
            match was.column(&column.name) {
                None if !column.nullable && column.default.is_none() => {
                    self.added_required(name, column, table);
                }
                None => self.statements.push(add_column(name, column)),
                Some(old) if old != column => {
                    self.changed_column(name, (old, column), table, renames);
                }
                Some(_) => {}
            }
        }
        for old in &was.columns {
            if table.column(&old.name).is_some() {
                continue;
            }
            if table.retired.columns.contains(&old.name) {
                self.dropped_columns.push(format!(
                    "ALTER TABLE {name} DROP COLUMN IF EXISTS {};",
                    old.name
                ));
            } else {
                self.refuse(
                    name,
                    format!(
                        "column `{}` is gone; retire it on the model to drop it, or say the column it became `was` it",
                        old.name
                    ),
                );
            }
        }
    }

    /// Whether the model of `table` passes `lint`: allowed, or warned of and accepted.
    fn passes(&self, lint: Lint, table: &SnapshotTable) -> bool {
        match self.lints.level(lint) {
            Level::Allow => true,
            Level::Warn => table.accepted.contains_key(&lint),
            Level::Refuse => false,
        }
    }

    /// A column retyped: `ALTER … TYPE` once `table_rewrite` and `narrowed_type` pass.
    fn retyped(
        &mut self,
        name: &str,
        (old, column): (&SnapshotColumn, &SnapshotColumn),
        table: &SnapshotTable,
    ) {
        let narrowed = crate::migration::lint::narrows(&old.data_type, &column.data_type);
        let refused: Vec<Lint> = [Lint::TableRewrite]
            .into_iter()
            .chain(narrowed.then_some(Lint::NarrowedType))
            .filter(|lint| !self.passes(*lint, table))
            .collect();
        if refused.is_empty() {
            self.statements.push(format!(
                "ALTER TABLE {name} ALTER COLUMN {} TYPE {};",
                column.name, column.data_type
            ));
            return;
        }
        let lints: Vec<String> = refused.into_iter().map(said).collect();
        self.refuse(
            name,
            format!(
                "{}: column `{}` changed its type from {} to {}; add a new column and move to \
                 it",
                lints.join(", "),
                column.name,
                old.data_type,
                column.data_type
            ),
        );
    }

    /// A column added `NOT NULL` with no default, once `not_null_without_default` passes.
    fn added_required(&mut self, name: &str, column: &SnapshotColumn, table: &SnapshotTable) {
        if self.passes(Lint::NotNullWithoutDefault, table) {
            self.statements.push(add_column(name, column));
            return;
        }
        self.refuse(
            name,
            format!(
                "{}: column `{}` is added NOT NULL with no default; give it a default or make it \
                 optional",
                said(Lint::NotNullWithoutDefault),
                column.name
            ),
        );
    }

    /// A column both sides hold that changed: retyped, made `NOT NULL`, or refused.
    fn changed_column(
        &mut self,
        name: &str,
        (old, column): (&SnapshotColumn, &SnapshotColumn),
        table: &SnapshotTable,
        renames: &[(String, String)],
    ) {
        let retyped = old.data_type != column.data_type
            && old.nullable == column.nullable
            && old.default == column.default;
        let made_required = old.data_type == column.data_type
            && old.nullable
            && !column.nullable
            && old.default == column.default;
        if retyped {
            self.retyped(name, (old, column), table);
            return;
        }
        if made_required {
            if self.passes(Lint::NotNullWithoutDefault, table) {
                self.statements.push(format!(
                    "ALTER TABLE {name} ALTER COLUMN {} SET NOT NULL;",
                    column.name
                ));
            } else {
                self.refuse(
                    name,
                    format!(
                        "{}: column `{}` is made NOT NULL, which fails on any row that holds \
                         none; backfill it and accept the lint, or add a new column",
                        said(Lint::NotNullWithoutDefault),
                        column.name
                    ),
                );
            }
            return;
        }
        let reason = match renames.iter().find(|(_, new)| *new == column.name) {
            Some((before, _)) => format!(
                "column `{}`, which was `{before}`, changed its type, nullability or default; rename it alone, then add a new column and move to it",
                column.name
            ),
            None => format!(
                "column `{}` changed its type, nullability or default; add a new column and move to it",
                column.name
            ),
        };
        self.refuse(name, reason);
    }

    fn checks(&mut self, name: &str, was: &SnapshotTable, table: &SnapshotTable) {
        for (check, expression) in &table.checks {
            if table.retired.checks.contains(check) {
                self.refuse(
                    name,
                    format!("check `{check}` is both declared and retired; keep one"),
                );
            }
            match was.checks.get(check) {
                None => self.statements.push(format!(
                    "ALTER TABLE {name} ADD CONSTRAINT {check} CHECK ({expression});"
                )),
                Some(old) if old != expression => self.refuse(
                    name,
                    format!(
                        "check `{check}` changed; add the new one under another name and retire the old"
                    ),
                ),
                Some(_) => {}
            }
        }
        for old in was.checks.keys() {
            if table.checks.contains_key(old) {
                continue;
            }
            if table.retired.checks.contains(old) {
                self.dropped_checks.push(format!(
                    "ALTER TABLE {name} DROP CONSTRAINT IF EXISTS {old};"
                ));
            } else {
                self.refuse(
                    name,
                    format!("check `{old}` is gone; retire it on the model to drop it"),
                );
            }
        }
    }

    /// Refuses a new constraint unique index: a constraint cannot say how its key is split.
    fn no_new_constraints<'a>(
        &mut self,
        name: &str,
        indexes: impl Iterator<Item = (&'a String, &'a SnapshotIndex)>,
    ) {
        for (index_name, _) in indexes.filter(|(_, index)| index.constraint) {
            self.refuse(
                name,
                format!(
                    "index `{index_name}` is a constraint's, which a generated migration never makes; declare it a unique index, whose key says how it is split"
                ),
            );
        }
    }

    fn indexes(&mut self, name: &str, was: &SnapshotTable, table: &SnapshotTable) {
        self.no_new_constraints(
            name,
            table
                .indexes
                .iter()
                .filter(|(index_name, _)| !was.indexes.contains_key(*index_name)),
        );
        for (index_name, index) in &table.indexes {
            if table.retired.indexes.contains(index_name) {
                self.refuse(
                    name,
                    format!("index `{index_name}` is both declared and retired; keep one"),
                );
            }
            match was.indexes.get(index_name) {
                None => self
                    .statements
                    .push(create_index(name, &index.schema(index_name))),
                Some(old) if old != index => self.refuse(
                    name,
                    format!(
                        "index `{index_name}` changed; give the new one another name and retire the old"
                    ),
                ),
                Some(_) => {}
            }
        }
        for (old, index) in &was.indexes {
            if table.indexes.contains_key(old) {
                continue;
            }
            if table.retired.indexes.contains(old) && index.constraint {
                self.dropped_indexes.push(format!(
                    "ALTER TABLE {name} DROP CONSTRAINT IF EXISTS {old};"
                ));
            } else if table.retired.indexes.contains(old) {
                self.dropped_indexes
                    .push(format!("DROP INDEX IF EXISTS {old};"));
            } else {
                self.refuse(
                    name,
                    format!("index `{old}` is gone; retire it on the model to drop it"),
                );
            }
        }
    }
}

fn said(lint: Lint) -> String {
    format!("{} {lint}", lint.code())
}

fn renamed_in(text: &str, renames: &[(String, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let mut quoted = false;
    let flush = |word: &mut String, out: &mut String| {
        match renames.iter().find(|(old, _)| old == word) {
            Some((_, new)) => out.push_str(new),
            None => out.push_str(word),
        }
        word.clear();
    };
    for c in text.chars() {
        if !quoted && (c.is_ascii_alphanumeric() || c == '_') {
            word.push(c);
            continue;
        }
        flush(&mut word, &mut out);
        if c == '\'' {
            quoted = !quoted;
        }
        out.push(c);
    }
    flush(&mut word, &mut out);
    out
}

fn add_column(table: &str, column: &SnapshotColumn) -> String {
    let not_null = if column.nullable { "" } else { " NOT NULL" };
    let default = column
        .default
        .as_deref()
        .map(|default| format!(" DEFAULT {default}"))
        .unwrap_or_default();
    format!(
        "ALTER TABLE {table} ADD COLUMN IF NOT EXISTS {} {}{not_null}{default};",
        column.name, column.data_type
    )
}

mod difference;
mod owner;
mod rebase;

pub use difference::{SchemaDifference, SchemaItem};
pub use owner::{
    BASE_VARIABLE, GenerateError, MODE_VARIABLE, NAME_VARIABLE, check, regenerate,
    the_models_are_their_migrations, write_baseline, write_next,
};
pub use rebase::{Main, Rebased, rebase};

#[cfg(test)]
mod tests;
