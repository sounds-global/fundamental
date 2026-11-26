//! [`Retirements`]: the drops and renames each owner declares, which the guard passes.

use std::collections::{BTreeMap, BTreeSet};

use fundamental_core::model::sql::Lint;

use crate::migration::ddl::column_renames;
use crate::migration::{EmbeddedMigrations, TableSchema, split_statements};

/// What each owner of embedded migrations declares it drops or renames, by its models and by
/// naming files: the one acknowledgement the guard takes for a destructive statement in that
/// owner's files.
///
/// Read from the owner's models (retired columns, indexes and checks, `was` renames, readers),
/// its `retired = […]` tables and its `drops_by_hand = […]` files, with each table's
/// acceptances. A retirement stays on the model, so applied files keep passing. A migration
/// is the owner's when its ledger name is `<owner>/<file>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Retirements {
    owners: BTreeMap<String, Declared>,
}

/// One owner's declarations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Declared {
    passed: BTreeSet<(String, String)>,
    by_hand: BTreeSet<String>,
    readers: BTreeMap<String, BTreeSet<String>>,
    accepted: BTreeMap<String, BTreeSet<Lint>>,
}

impl Declared {
    fn passes(&self, verb: &str, object: &str) -> bool {
        self.passed.contains(&(verb.to_owned(), object.to_owned()))
            || (verb == "DROP POLICY" && self.reader_gone(object))
    }

    /// Whether `object` (`<table>.<policy>`) is a generated reader policy whose role the model no
    /// longer names.
    fn reader_gone(&self, object: &str) -> bool {
        let Some((table, policy)) = object.split_once('.') else {
            return false;
        };
        let Some(role) = policy.strip_prefix(&format!("{table}_read_by_")) else {
            return false;
        };
        self.readers
            .get(table)
            .is_some_and(|now| !role.is_empty() && !now.contains(role))
    }

    fn pass(&mut self, verb: &str, object: String) {
        self.passed.insert((verb.to_owned(), object));
    }

    /// What `set` declares by its models, retired tables, renames and `drops_by_hand`.
    fn declare(&mut self, set: &EmbeddedMigrations) {
        for table in set.schemas() {
            self.declare_table(&table);
        }
        for table in set.retired_tables() {
            self.pass("DROP TABLE", (*table).to_owned());
        }
        // Newest first, so a rename renamed again later is judged after that later rename.
        let renames = set
            .migrations()
            .iter()
            .flat_map(|migration| split_statements(migration.up()))
            .flat_map(|statement| column_renames(&statement))
            .collect::<Vec<_>>();
        for (table, before, after) in renames.into_iter().rev() {
            let after = format!("{table}.{after}");
            if self.passes("DROP COLUMN", &after) || self.passes("RENAME COLUMN", &after) {
                self.pass("RENAME COLUMN", format!("{table}.{before}"));
            }
        }
        self.by_hand.extend(
            set.files_dropping_by_hand()
                .iter()
                .map(|file| (*file).to_owned()),
        );
    }

    /// What one model retires and renames on its table.
    fn declare_table(&mut self, table: &TableSchema) {
        let name = &table.name;
        for column in &table.retired.columns {
            self.pass("DROP COLUMN", format!("{name}.{column}"));
        }
        for check in &table.retired.checks {
            self.pass("DROP CONSTRAINT", format!("{name}.{check}"));
        }
        for index in &table.retired.indexes {
            self.pass("DROP CONSTRAINT", format!("{name}.{index}"));
            self.pass("DROP INDEX", index.clone());
        }
        for old in table.renamed.values() {
            self.pass("RENAME COLUMN", format!("{name}.{old}"));
        }
        if table.readers.is_empty() {
            self.pass("DROP POLICY", format!("{name}.{name}_written"));
        }
        self.readers
            .insert(name.clone(), table.readers.keys().cloned().collect());
        self.accepted
            .insert(name.clone(), table.accepted.keys().copied().collect());
    }
}

impl Retirements {
    /// No declarations: every destructive statement needs an
    /// [`Exemption::drop_of`](super::Exemption::drop_of).
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The declarations of `sets`.
    #[must_use]
    pub fn of<'a>(sets: impl IntoIterator<Item = &'a EmbeddedMigrations>) -> Self {
        let mut owners: BTreeMap<String, Declared> = BTreeMap::new();
        for set in sets {
            owners
                .entry(set.owner().to_owned())
                .or_default()
                .declare(set);
        }
        Self { owners }
    }

    /// Whether `owner`'s models or retired tables declare `verb` on `object`.
    pub(crate) fn declared_by_models(&self, owner: &str, verb: &str, object: &str) -> bool {
        self.owners
            .get(owner)
            .is_some_and(|declared| declared.passes(verb, object))
    }

    /// Whether the owner of `migration` (`<owner>/<file>`) accepts `lint` on `table`.
    #[must_use]
    pub fn accepts(&self, migration: &str, lint: Lint, table: &str) -> bool {
        let Some((owner, _)) = migration.split_once('/') else {
            return false;
        };
        self.owners.get(owner).is_some_and(|declared| {
            declared
                .accepted
                .get(table)
                .is_some_and(|accepted| accepted.contains(&lint))
        })
    }

    /// Whether the owner of `migration` (`<owner>/<file>`) declares `verb` on `object`, or names
    /// the file in `drops_by_hand`.
    #[must_use]
    pub fn passes(&self, migration: &str, verb: &str, object: &str) -> bool {
        let Some((owner, file)) = migration.split_once('/') else {
            return false;
        };
        self.owners.get(owner).is_some_and(|declared| {
            declared.by_hand.contains(file) || declared.passes(verb, object)
        })
    }
}
