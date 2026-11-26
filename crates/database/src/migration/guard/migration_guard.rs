//! [`MigrationGuard`]: what a migration set must satisfy before any of it runs.

use std::collections::BTreeMap;

use tracing::{debug, instrument};

use crate::migration::ddl::{DdlStatement, KeyDeclaration, recognise_all};
use crate::migration::lint::{Lint, LintFinding, Lints, judge, report, verdict};
use crate::migration::split_statements;

use super::{Exemption, GuardError, Retirements};

/// The policy a migration set is held to, applied before its first statement executes.
///
/// An undeclared key is not unset: the engine hashes its leading column, which can put a
/// whole cell on one tablet or scatter every ordered read. So the text, the only artefact
/// before the schema exists, must declare it. Past those fixed rules, every [`Lint`] is
/// judged at its [`Lints`] level and every blocking finding reported at once
/// ([`GuardError::Linted`]); warnings pass by a model's acceptance, [`Exemption::accept`],
/// [`Retirements`] or [`Exemption::drop_of`].
#[derive(Debug, Clone)]
pub struct MigrationGuard {
    enforced: bool,
    min_hash_fanout: u64,
    lints: Lints,
    exempt: &'static [Exemption],
}

impl MigrationGuard {
    /// Declares the policy.
    ///
    /// `min_hash_fanout` is the distinct values a hash component must be able to take; it bites
    /// only when the migration's own `CHECK (col IN (…))` bounds it below. `lints` are the
    /// project's levels, from `lints!()?`. Every field is required.
    #[must_use]
    pub const fn new(min_hash_fanout: u64, lints: Lints, exempt: &'static [Exemption]) -> Self {
        Self {
            enforced: true,
            min_hash_fanout,
            lints,
            exempt,
        }
    }

    /// The levels it holds the lints to.
    #[must_use]
    pub const fn lints(&self) -> &Lints {
        &self.lints
    }

    /// A guard that enforces nothing.
    ///
    /// A named escape for fixtures whose subject is something else; never for a deployed
    /// migration path.
    #[must_use]
    pub fn unenforced() -> Self {
        Self {
            enforced: false,
            min_hash_fanout: 0,
            lints: Lints::default(),
            exempt: &[],
        }
    }

    /// Judges a whole SQL migration set — sharding **and** safety.
    ///
    /// Runs before any statement executes, so a refused set cannot half-apply.
    ///
    /// # Errors
    /// Returns the first fixed rule's [`GuardError`] found, naming the migration, the object
    /// and the text at fault; else [`GuardError::Linted`] with every lint finding that blocks.
    ///
    /// No owner's declarations pass a destructive statement here;
    /// [`inspect_sql_passing`](Self::inspect_sql_passing) takes them.
    pub fn inspect_sql(&self, migrations: &[(&str, &str)]) -> Result<(), GuardError> {
        self.inspect_sql_passing(migrations, &Retirements::none())
    }

    /// Judges a whole SQL migration set as [`inspect_sql`](Self::inspect_sql) does, passing a
    /// destructive statement in an owner's file (`<owner>/<file>`) that the owner's models or
    /// its `drops_by_hand` declare ([`Retirements`]).
    ///
    /// # Errors
    /// Returns the first fixed rule's [`GuardError`] found, naming the migration, the object
    /// and the text at fault; else [`GuardError::Linted`] with every lint finding that blocks.
    #[instrument(skip(self, migrations, retirements), fields(migrations = migrations.len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn inspect_sql_passing(
        &self,
        migrations: &[(&str, &str)],
        retirements: &Retirements,
    ) -> Result<(), GuardError> {
        for (name, sql) in migrations {
            self.inspect_one(name, sql, true)?;
        }
        self.lint(migrations, true, retirements)?;
        debug!("migration set accepted: every key declares its sharding, and no lint blocks");
        Ok(())
    }

    fn lint(
        &self,
        migrations: &[(&str, &str)],
        sharded: bool,
        retirements: &Retirements,
    ) -> Result<(), GuardError> {
        if !self.enforced {
            return Ok(());
        }
        let found = judge::findings(migrations, &self.lints, sharded);
        let findings = verdict::blocking(found, &self.lints, |finding| {
            self.accepts(finding, retirements)
        });
        if findings.is_empty() {
            return Ok(());
        }
        Err(GuardError::Linted {
            report: report(&findings),
            findings,
        })
    }

    fn accepts(&self, finding: &LintFinding, retirements: &Retirements) -> bool {
        let migration = finding.migration.as_deref().unwrap_or_default();
        Exemption::find_accept(self.exempt, finding.lint, &finding.table).is_some()
            || retirements.accepts(migration, finding.lint, &finding.table)
            || (finding.lint == Lint::Destructive
                && (Exemption::find_drop(self.exempt, &finding.verb, &finding.object).is_some()
                    || retirements.passes(migration, &finding.verb, &finding.object)))
    }

    /// Judges a whole CQL or offline migration set — safety only.
    ///
    /// Neither wire has the SQL sharding vocabulary, so only the `destructive` and
    /// `not_null_without_default` lints are judged.
    ///
    /// # Errors
    /// Returns the first [`GuardError`] found, or [`GuardError::Linted`].
    #[instrument(skip(self, migrations), fields(migrations = migrations.len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn inspect_unsharded(&self, migrations: &[(&str, &str)]) -> Result<(), GuardError> {
        for (name, sql) in migrations {
            self.inspect_one(name, sql, false)?;
        }
        self.lint(migrations, false, &Retirements::none())
    }

    fn inspect_one(&self, migration: &str, sql: &str, sharded: bool) -> Result<(), GuardError> {
        if !self.enforced {
            return Ok(());
        }
        for statement in split_statements(sql) {
            let recognised =
                recognise_all(&statement).map_err(|source| GuardError::Unreadable {
                    migration: migration.to_owned(),
                    source,
                })?;
            for one in &recognised {
                self.judge(migration, one, sharded)?;
            }
        }
        Ok(())
    }

    /// One statement, or one action of an `ALTER TABLE`, by the fixed rules.
    fn judge(
        &self,
        migration: &str,
        recognised: &DdlStatement,
        sharded: bool,
    ) -> Result<(), GuardError> {
        match recognised {
            DdlStatement::CreateTable {
                table,
                primary_key,
                inline_primary_key_columns,
                unique_constraints,
                value_bounds,
                ..
            } if sharded && !self.is_exempt(table) => {
                self.judge_table_shape(
                    migration,
                    table,
                    inline_primary_key_columns,
                    unique_constraints,
                )?;
                if let Some(key) = primary_key {
                    self.judge_key(migration, table, key, value_bounds)?;
                }
            }
            DdlStatement::AddPrimaryKey { table, key } if sharded && !self.is_exempt(table) => {
                self.judge_key(migration, table, key, &BTreeMap::new())?;
            }
            DdlStatement::CreateIndex { name, key, .. } if sharded && !self.is_exempt(name) => {
                // An index's column bounds live on the table, which this statement does not see.
                self.judge_key(migration, name, key, &BTreeMap::new())?;
            }
            _ => {}
        }
        Ok(())
    }

    /// A sharded table carries no inline primary key and no UNIQUE constraint.
    fn judge_table_shape(
        &self,
        migration: &str,
        table: &str,
        inline_primary_key_columns: &[String],
        unique_constraints: &[Vec<String>],
    ) -> Result<(), GuardError> {
        if let Some(column) = inline_primary_key_columns.first() {
            return Err(GuardError::InlinePrimaryKey {
                migration: migration.to_owned(),
                table: table.to_owned(),
                column: column.clone(),
            });
        }
        if let Some(columns) = unique_constraints.first() {
            return Err(GuardError::UniqueConstraint {
                migration: migration.to_owned(),
                table: table.to_owned(),
                columns: columns.join(", "),
            });
        }
        Ok(())
    }

    fn judge_key(
        &self,
        migration: &str,
        object: &str,
        key: &KeyDeclaration,
        bounds: &BTreeMap<String, u64>,
    ) -> Result<(), GuardError> {
        if let Some(element) = key.undeclared().first() {
            return Err(GuardError::UndeclaredKey {
                migration: migration.to_owned(),
                object: object.to_owned(),
                element: element.to_string(),
            });
        }

        let hash = key.hash_columns();
        if hash.is_empty() {
            return Ok(());
        }

        // The fan-out is the product of the columns' bounds, unbounded if any column is.
        let mut fanout: u64 = 1;
        for column in hash {
            let Some(bound) = bounds.get(column) else {
                return Ok(());
            };
            fanout = fanout.saturating_mul(*bound);
        }

        if fanout < self.min_hash_fanout {
            return Err(GuardError::HotHashComponent {
                migration: migration.to_owned(),
                object: object.to_owned(),
                columns: hash.join(", "),
                bound: fanout,
                floor: self.min_hash_fanout,
            });
        }
        Ok(())
    }

    fn is_exempt(&self, object: &str) -> bool {
        Exemption::find(self.exempt, object).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::MigrationGuard;
    use crate::migration::guard::{Exemption, GuardError};
    use crate::migration::{Level, Lint, Lints};

    const NONE: &[Exemption] = &[];

    fn strict() -> MigrationGuard {
        MigrationGuard::new(1_000, Lints::default(), NONE)
    }

    /// Whether `error` is the lints refusing the set for `lint`.
    pub(super) fn linted(error: &GuardError, lint: Lint) -> bool {
        error.lints().contains(&lint)
    }

    #[test]
    fn a_fully_declared_migration_is_accepted() {
        let sql = "CREATE TABLE permission_grants (\n\
             zone TEXT NOT NULL,\n\
             grant_id TEXT NOT NULL,\n\
             seq INT8 NOT NULL,\n\
             PRIMARY KEY ((zone, grant_id) HASH, seq ASC)\n\
             );\n\
             CREATE INDEX permission_grants_due ON permission_grants (zone ASC, seq ASC);";

        strict()
            .inspect_sql(&[("0001_grants", sql)])
            .expect("every element declares its sharding");
    }

    #[test]
    fn an_undeclared_key_element_is_refused_by_name() {
        let sql = "CREATE TABLE t (zone TEXT NOT NULL, id TEXT NOT NULL, \
                   PRIMARY KEY (zone, id));";

        let error = strict()
            .inspect_sql(&[("0001_t", sql)])
            .expect_err("refused");
        let GuardError::UndeclaredKey {
            object, element, ..
        } = error
        else {
            panic!("expected UndeclaredKey, got {error:?}");
        };
        assert_eq!(object, "t");
        assert_eq!(element, "zone");
    }

    #[test]
    fn a_key_added_by_alter_declares_its_sharding_as_a_created_one_must() {
        let sql = "CREATE TABLE t (id TEXT NOT NULL);\nALTER TABLE t ADD PRIMARY KEY (id);";

        let error = strict()
            .inspect_sql(&[("0001_t", sql)])
            .expect_err("refused");
        let GuardError::UndeclaredKey {
            object, element, ..
        } = error
        else {
            panic!("expected UndeclaredKey, got {error:?}");
        };
        assert_eq!((object.as_str(), element.as_str()), ("t", "id"));
    }

    #[test]
    fn a_key_added_to_a_table_with_rows_is_refused_by_default() {
        let error = strict()
            .inspect_sql(&[
                (
                    "0001_t",
                    "CREATE TABLE t (id TEXT NOT NULL, at TIMESTAMPTZ NOT NULL, \
                     PRIMARY KEY (id HASH));",
                ),
                (
                    "0002_t_key",
                    "ALTER TABLE t ADD PRIMARY KEY (id HASH, at ASC);",
                ),
            ])
            .expect_err("refused");

        assert!(linted(&error, Lint::KeySplitChanged), "{error:?}");
    }

    #[test]
    fn an_undeclared_index_element_is_refused_even_beside_a_declared_one() {
        let sql = "CREATE INDEX c_by_witness ON c (witness_name, tree_size DESC);";

        let error = strict()
            .inspect_sql(&[("0003_c", sql)])
            .expect_err("refused");
        let GuardError::UndeclaredKey { element, .. } = error else {
            panic!("expected UndeclaredKey, got {error:?}");
        };
        assert_eq!(element, "witness_name");
    }

    #[test]
    fn an_inline_primary_key_is_refused_because_it_cannot_declare() {
        let sql = "CREATE TABLE audit_records (id UUID PRIMARY KEY, kind TEXT NOT NULL);";

        let error = strict()
            .inspect_sql(&[("0001_a", sql)])
            .expect_err("refused");
        assert!(
            matches!(error, GuardError::InlinePrimaryKey { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_unique_constraint_is_refused_in_favour_of_a_declared_index() {
        let sql = "CREATE TABLE t (id UUID NOT NULL, c TEXT NOT NULL UNIQUE, \
                   PRIMARY KEY (id HASH));";

        let error = strict()
            .inspect_sql(&[("0001_t", sql)])
            .expect_err("refused");
        assert!(
            matches!(error, GuardError::UniqueConstraint { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_hash_component_its_own_check_proves_hot_is_refused() {
        let sql = "CREATE TABLE t (\n\
             kind TEXT NOT NULL CHECK (kind IN ('a', 'b', 'c')),\n\
             id TEXT NOT NULL,\n\
             PRIMARY KEY (kind HASH, id ASC)\n\
             );";

        let error = strict()
            .inspect_sql(&[("0001_t", sql)])
            .expect_err("refused");
        let GuardError::HotHashComponent { bound, floor, .. } = error else {
            panic!("expected HotHashComponent, got {error:?}");
        };
        assert_eq!(bound, 3);
        assert_eq!(floor, 1_000);
    }

    #[test]
    fn one_unbounded_column_makes_the_component_unbounded() {
        let sql = "CREATE TABLE t (\n\
             kind TEXT NOT NULL CHECK (kind IN ('a', 'b', 'c')),\n\
             id TEXT NOT NULL,\n\
             PRIMARY KEY ((kind, id) HASH)\n\
             );";

        strict()
            .inspect_sql(&[("0001_t", sql)])
            .expect("`id` is unbounded, so the pair spreads");
    }

    #[test]
    fn a_destructive_statement_is_refused_under_the_refusing_policy() {
        let sql = "DROP TABLE widgets;";

        let error = strict()
            .inspect_sql(&[("0009_d", sql)])
            .expect_err("refused");
        assert!(linted(&error, Lint::Destructive), "{error:?}");
    }

    #[test]
    fn the_allowing_policy_lets_the_same_statement_through() {
        let guard = MigrationGuard::new(
            1_000,
            Lints::default().with(Lint::Destructive, Level::Allow),
            NONE,
        );

        guard
            .inspect_sql(&[("0009_d", "DROP TABLE widgets;")])
            .expect("the exception was stated");
    }

    const DROPS_ONE_INDEX: &[Exemption] = &[Exemption::drop_of(
        "DROP INDEX",
        "old_unique",
        "replaced in the same migration by a wider unique index",
    )];

    fn names_one_drop() -> MigrationGuard {
        MigrationGuard::new(1_000, Lints::default(), DROPS_ONE_INDEX)
    }

    #[test]
    fn a_named_drop_passes_with_its_reason() {
        names_one_drop()
            .inspect_sql(&[("0009_d", "DROP INDEX IF EXISTS old_unique;")])
            .expect("the exception names this drop");
    }

    #[test]
    fn a_named_drop_passes_no_other_object() {
        let error = names_one_drop()
            .inspect_sql(&[("0009_d", "DROP INDEX other_unique;")])
            .expect_err("refused");
        assert!(linted(&error, Lint::Destructive), "{error:?}");
    }

    #[test]
    fn a_named_drop_passes_no_other_verb() {
        let error = names_one_drop()
            .inspect_sql(&[("0009_d", "DROP TABLE old_unique;")])
            .expect_err("refused");
        assert!(linted(&error, Lint::Destructive), "{error:?}");
    }

    #[test]
    fn a_key_exemption_passes_no_drop() {
        const KEY: &[Exemption] = &[Exemption::new("old_unique", "its key is judged elsewhere")];
        let guard = MigrationGuard::new(1_000, Lints::default(), KEY);

        let error = guard
            .inspect_sql(&[("0009_d", "DROP INDEX old_unique;")])
            .expect_err("refused");
        assert!(linted(&error, Lint::Destructive), "{error:?}");
    }

    #[test]
    fn a_named_drop_exempts_no_key() {
        let sql = "CREATE INDEX old_unique ON widgets (team);";

        let error = names_one_drop()
            .inspect_sql(&[("0009_d", sql)])
            .expect_err("the index still declares its key");
        assert!(!linted(&error, Lint::Destructive), "{error:?}");
    }

    #[test]
    fn a_not_null_column_with_no_default_cannot_be_backfilled() {
        let sql = "ALTER TABLE widgets ADD COLUMN owner TEXT NOT NULL;";

        let error = strict()
            .inspect_sql(&[("0010_w", sql)])
            .expect_err("refused");
        assert!(linted(&error, Lint::NotNullWithoutDefault), "{error:?}");
    }

    #[test]
    fn an_exempt_object_passes_and_its_reason_is_carried() {
        const EXEMPT: &[Exemption] = &[Exemption::new(
            "legacy_widgets",
            "frozen: read-only, dropped at the next fold",
        )];
        let guard = MigrationGuard::new(1_000, Lints::default(), EXEMPT);

        guard
            .inspect_sql(&[(
                "0001_l",
                "CREATE TABLE legacy_widgets (id TEXT NOT NULL, PRIMARY KEY (id));",
            )])
            .expect("exempt");
    }

    #[test]
    fn the_unsharded_pass_judges_safety_but_not_sharding() {
        let guard = strict();
        let undeclared = "CREATE TABLE t (id TEXT NOT NULL, PRIMARY KEY (id));";

        guard
            .inspect_unsharded(&[("0001_t", undeclared)])
            .expect("CQL has no HASH/ASC/DESC on a primary key");
        guard
            .inspect_unsharded(&[("0002_t", "DROP TABLE t;")])
            .expect_err("safety still applies");
    }

    #[test]
    fn an_unenforced_guard_judges_nothing_at_all() {
        let guard = MigrationGuard::unenforced();

        guard
            .inspect_sql(&[
                (
                    "0001_t",
                    "CREATE TABLE t (id TEXT NOT NULL, PRIMARY KEY (id));",
                ),
                ("0002_t", "DROP TABLE t;"),
                ("0003_broken", "CREATE TABLE widgets"),
            ])
            .expect("a fixture's schema is not this guard's subject");
    }

    mod declared {
        use super::{linted, strict};
        use crate::migration::guard::{Exemption, GuardError, Retirements};
        use crate::migration::{
            EmbeddedMigration, EmbeddedMigrations, MigrationGuard, TableSchema,
        };
        use crate::migration::{Lint, Lints};

        fn widgets() -> TableSchema {
            let mut table = TableSchema::new("widgets");
            table.retired.columns = vec!["legacy".to_owned(), "publish_text".to_owned()];
            table.retired.checks = vec!["widgets_kind".to_owned()];
            table.retired.indexes = vec!["widgets_by_kind".to_owned()];
            table
                .renamed
                .insert("visibility".to_owned(), "state".to_owned());
            table
        }

        static FILES: [EmbeddedMigration; 2] = [
            EmbeddedMigration::new(
                "0002_publish_text",
                "ALTER TABLE widgets RENAME COLUMN publish_at TO publish_text;",
            ),
            EmbeddedMigration::new("0003_featured", "ALTER TABLE widgets DROP COLUMN featured;"),
        ];
        static ACME: EmbeddedMigrations =
            EmbeddedMigrations::sql("acme", &FILES, &[widgets as fn() -> TableSchema])
                .retired(&["gadgets"])
                .dropping_by_hand(&["0003_featured"]);

        fn judged(migration: &str, sql: &str) -> Result<(), GuardError> {
            strict().inspect_sql_passing(&[(migration, sql)], &Retirements::of([&ACME]))
        }

        #[test]
        fn what_an_owners_models_retire_passes_in_its_files() {
            for sql in [
                "ALTER TABLE widgets DROP COLUMN IF EXISTS legacy;",
                "ALTER TABLE widgets DROP CONSTRAINT IF EXISTS widgets_kind;",
                "ALTER TABLE widgets DROP CONSTRAINT IF EXISTS widgets_by_kind;",
                "DROP INDEX IF EXISTS widgets_by_kind;",
                "DROP TABLE IF EXISTS gadgets;",
                "ALTER TABLE widgets RENAME COLUMN state TO visibility;",
            ] {
                judged("acme/0004_generated", sql).unwrap_or_else(|e| panic!("{sql}: {e:?}"));
            }
        }

        #[test]
        fn a_rename_to_a_name_later_retired_is_declared_by_that_retirement() {
            judged(
                "acme/0002_publish_text",
                "ALTER TABLE widgets RENAME COLUMN publish_at TO publish_text;",
            )
            .expect("publish_text is retired, so the name publish_at is gone as declared");
        }

        #[test]
        fn what_no_model_declares_is_refused_even_in_the_owners_files() {
            for sql in [
                "ALTER TABLE widgets DROP COLUMN owner;",
                "ALTER TABLE widgets RENAME COLUMN owner TO holder;",
                "ALTER TABLE widgets ADD COLUMN note TEXT, DROP COLUMN owner;",
                "DROP TABLE widgets;",
            ] {
                let error = judged("acme/0004_generated", sql).expect_err(sql);
                assert!(linted(&error, Lint::Destructive), "{error:?}");
            }
        }

        #[test]
        fn one_owners_declarations_pass_nothing_of_another_or_of_the_app() {
            let sql = "ALTER TABLE widgets DROP COLUMN legacy;";
            for migration in ["other/0004_generated", "0004_generated"] {
                let error = judged(migration, sql).expect_err(migration);
                assert!(linted(&error, Lint::Destructive), "{error:?}");
            }
        }

        #[test]
        fn a_file_named_to_drop_by_hand_passes_and_no_other_file_does() {
            judged(
                "acme/0003_featured",
                "ALTER TABLE widgets DROP COLUMN featured;",
            )
            .expect("named");
            judged(
                "acme/0004_more",
                "ALTER TABLE widgets DROP COLUMN featured;",
            )
            .expect_err("not named");
        }

        #[test]
        fn a_reader_no_model_names_any_more_is_dropped_as_declared() {
            // `widgets` has no reader now: its models took `read_by` off.
            for sql in [
                "DROP POLICY IF EXISTS widgets_read_by_www ON widgets;",
                "DROP POLICY IF EXISTS widgets_written ON widgets;",
            ] {
                judged("acme/0004_generated", sql).unwrap_or_else(|e| panic!("{sql}: {e:?}"));
            }
        }

        fn read_by_www() -> TableSchema {
            let mut table = TableSchema::new("widgets");
            table.add_reader("www", "state = 'published'");
            table
        }

        static READ: EmbeddedMigrations =
            EmbeddedMigrations::sql("acme", &[], &[read_by_www as fn() -> TableSchema]);

        #[test]
        fn a_policy_a_model_still_holds_or_one_the_generator_never_wrote_is_refused() {
            for (owner, sql) in [
                (
                    &READ,
                    "DROP POLICY IF EXISTS widgets_read_by_www ON widgets;",
                ),
                (&READ, "DROP POLICY IF EXISTS widgets_written ON widgets;"),
                (&ACME, "DROP POLICY IF EXISTS widgets_audit ON widgets;"),
                (&ACME, "DROP POLICY IF EXISTS gizmos_written ON gizmos;"),
            ] {
                let error = strict()
                    .inspect_sql_passing(&[("acme/0004_generated", sql)], &Retirements::of([owner]))
                    .expect_err(sql);
                assert!(linted(&error, Lint::Destructive), "{error:?}");
            }
        }

        #[test]
        fn an_apps_own_drop_passes_by_its_exemption() {
            const DROPS: &[Exemption] = &[Exemption::drop_of(
                "DROP COLUMN",
                "widgets.legacy",
                "moved to widget_notes in 0007",
            )];
            MigrationGuard::new(1_000, Lints::default(), DROPS)
                .inspect_sql(&[("0008_w", "ALTER TABLE widgets DROP COLUMN legacy;")])
                .expect("the exception names it");
        }

        #[test]
        fn without_declarations_every_drop_is_refused() {
            let error = strict()
                .inspect_sql(&[(
                    "acme/0004_generated",
                    "ALTER TABLE widgets DROP COLUMN legacy;",
                )])
                .expect_err("refused");
            assert!(linted(&error, Lint::Destructive), "{error:?}");
        }
    }

    #[test]
    fn every_lint_that_blocks_is_reported_at_once_with_its_code_and_fix() {
        let error = strict()
            .inspect_sql(&[(
                "0010_w",
                "ALTER TABLE widgets ADD COLUMN owner TEXT NOT NULL;\n\
                 ALTER TABLE widgets DROP COLUMN legacy;",
            )])
            .expect_err("refused");

        assert_eq!(
            error.lints(),
            [Lint::NotNullWithoutDefault, Lint::Destructive],
            "in the order the statements come"
        );
        let said = error.to_string();
        assert!(
            said.contains("MIG_L01 destructive  0010_w: DROP COLUMN widgets.legacy"),
            "{said}"
        );
        assert!(said.contains("MIG_L09 not_null_without_default"), "{said}");
        assert!(said.contains(Lint::NotNullWithoutDefault.fix()), "{said}");
    }

    #[test]
    fn a_warning_in_the_apps_own_files_passes_by_its_acceptance() {
        const ACCEPTS: &[Exemption] = &[Exemption::accept(
            Lint::HotShard,
            "events",
            "read newest first, written once a second by one importer",
        )];
        let sql = "CREATE TABLE events (at TIMESTAMPTZ NOT NULL, id UUID NOT NULL, \
                   PRIMARY KEY (at DESC, id ASC));";

        let refused = strict()
            .inspect_sql(&[("0001_events", sql)])
            .expect_err("hot");
        assert!(linted(&refused, Lint::HotShard), "{refused:?}");
        MigrationGuard::new(1_000, Lints::default(), ACCEPTS)
            .inspect_sql(&[("0001_events", sql)])
            .expect("accepted");
    }

    #[test]
    fn a_refused_lint_blocks_what_its_owner_declares_and_an_allowed_one_nothing() {
        let refusing = MigrationGuard::new(
            1_000,
            Lints::default().with(Lint::Destructive, Level::Refuse),
            NONE,
        );
        const DROPS: &[Exemption] = &[Exemption::drop_of(
            "DROP TABLE",
            "widgets",
            "moved to gadgets in 0008",
        )];
        let refusing_named = MigrationGuard::new(
            1_000,
            Lints::default().with(Lint::Destructive, Level::Refuse),
            DROPS,
        );

        refusing
            .inspect_sql(&[("0009_d", "DROP TABLE widgets;")])
            .expect_err("refused");
        let error = refusing_named
            .inspect_sql(&[("0009_d", "DROP TABLE widgets;")])
            .expect_err("no way out");
        assert!(
            error.to_string().contains("cannot pass a refused lint"),
            "{error}"
        );
    }

    #[test]
    fn a_statement_that_cannot_be_read_is_refused_rather_than_passed() {
        let error = strict()
            .inspect_sql(&[("0001_broken", "CREATE TABLE widgets")])
            .expect_err("unreadable");

        assert!(matches!(error, GuardError::Unreadable { .. }), "{error:?}");
    }
}
