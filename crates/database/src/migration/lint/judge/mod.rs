//! The lint judges, one a file, and the run that reads a migration set through them; levels
//! and acceptances are the [verdict](super::verdict)'s.

mod backfill;
mod blocking_index;
mod ddl_beside_data;
mod destructive;
mod hot_shard;
mod key_split_changed;
mod large_colocated;
mod narrowed_type;
mod not_null_without_default;
mod table_rewrite;
mod tenant_not_leading;
mod unindexed_reference;

use std::collections::BTreeSet;

pub(crate) use narrowed_type::narrows;

use super::facts::Facts;
use super::{LintFinding, Lints};
use crate::migration::ddl::{DdlStatement, recognise_all};
use crate::migration::split_statements;

/// What a judge of one statement is told: the schema before it, and its file.
pub(crate) struct Before<'a> {
    pub facts: &'a Facts,
    pub created_here: &'a BTreeSet<String>,
    pub migration: &'a str,
}

impl Before<'_> {
    /// A finding of `lint` in this file, at a statement of `verb`, on the table `statement`
    /// works on.
    pub fn finding(
        &self,
        lint: super::Lint,
        statement: &DdlStatement,
        verb: &str,
        at: impl Into<String>,
    ) -> LintFinding {
        let table = self.facts.table_of(statement).unwrap_or_default();
        LintFinding::new(lint, table.clone(), table, at)
            .in_file(self.migration)
            .by(verb)
    }
}

/// Every lint that fires in `files`, whatever the project's levels. A non-`sharded` (CQL or
/// SQLite) set is judged only for what removes data or fails on a table with rows.
pub(crate) fn findings(files: &[(&str, &str)], lints: &Lints, sharded: bool) -> Vec<LintFinding> {
    let mut facts = Facts::default();
    let mut found = Vec::new();
    for (migration, sql) in files {
        let statements: Vec<DdlStatement> = split_statements(sql)
            .iter()
            .filter_map(|statement| recognise_all(statement).ok())
            .flatten()
            .collect();
        let mut created_here = BTreeSet::new();
        for statement in &statements {
            let before = Before {
                facts: &facts,
                created_here: &created_here,
                migration,
            };
            found.extend(destructive::judge(&before, statement));
            found.extend(not_null_without_default::judge(&before, statement));
            if sharded {
                found.extend(table_rewrite::judge(&before, statement));
                found.extend(key_split_changed::judge(&before, statement));
                found.extend(narrowed_type::judge(&before, statement));
                found.extend(blocking_index::judge(&before, statement));
                found.extend(hot_shard::judge(&before, lints, statement));
                found.extend(backfill::judge(&before, statement));
            }
            facts.apply(migration, statement);
            if let DdlStatement::CreateTable { table, .. } = statement {
                created_here.insert(table.clone());
            }
        }
        if sharded {
            found.extend(ddl_beside_data::judge(migration, &statements));
        }
    }
    if sharded {
        found.extend(unindexed_reference::judge(&facts));
        found.extend(tenant_not_leading::judge(&facts, lints));
        found.extend(large_colocated::judge(&facts, lints));
    }
    found
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::super::{Lint, LintFinding, Lints};

    /// What `lint` finds in `files`, a YSQL set, under `lints`.
    pub fn fired_under(lints: &Lints, lint: Lint, files: &[(&str, &str)]) -> Vec<LintFinding> {
        super::findings(files, lints, true)
            .into_iter()
            .filter(|finding| finding.lint == lint)
            .collect()
    }

    /// What `lint` finds in `files` under the default lints.
    pub fn fired(lint: Lint, files: &[(&str, &str)]) -> Vec<LintFinding> {
        fired_under(&Lints::default(), lint, files)
    }

    pub const ORDERS: (&str, &str) = (
        "0001_orders",
        "CREATE TABLE orders (id UUID NOT NULL, total INT8 NOT NULL, note TEXT, \
         at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (id HASH));",
    );
}
