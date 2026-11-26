//! [`EmbeddedMigrations`]: the migrations and models a crate ships for the tables it owns.

use super::{EmbeddedMigration, MigrationDatabase, TableSchema};

/// The tables a crate owns, as migrations and the models they must match, shipped with the
/// crate rather than copied into each app.
///
/// Registered with the app's migrator, they run in the app's ledger as `<owner>/<name>`, are
/// hash-checked like any applied migration, and the models join the drift check.
#[derive(Clone, Copy, Debug)]
pub struct EmbeddedMigrations {
    owner: &'static str,
    database: MigrationDatabase,
    migrations: &'static [EmbeddedMigration],
    tables: &'static [fn() -> TableSchema],
    companions: &'static [fn() -> Option<TableSchema>],
    generated: Option<Generated>,
    retired: &'static [&'static str],
    needs_createrole: &'static [&'static str],
    drops_by_hand: &'static [&'static str],
}

/// Where a set generated from its models keeps what the generator wrote: the snapshot as the
/// crate was built with it, and the crate's directory, to write the next file into.
#[derive(Clone, Copy, Debug)]
pub struct Generated {
    /// The committed `migrations/schema.json`, as built in.
    pub snapshot: &'static str,
    /// The crate's directory: `migrations/` is written below it.
    pub dir: &'static str,
}

impl EmbeddedMigrations {
    /// A set for the SQL wire, owned by `owner`, applied in the order given.
    ///
    /// `owner` names the crate and follows the migration naming rule; `tables` are the models the
    /// migrations create, for the drift check.
    #[must_use]
    pub const fn sql(
        owner: &'static str,
        migrations: &'static [EmbeddedMigration],
        tables: &'static [fn() -> TableSchema],
    ) -> Self {
        Self {
            owner,
            database: MigrationDatabase::SqlDb,
            migrations,
            tables,
            companions: &[],
            generated: None,
            retired: &[],
            needs_createrole: &[],
            drops_by_hand: &[],
        }
    }

    /// It generated from its models: its committed snapshot, and the crate's directory the next
    /// file is written into.
    #[must_use]
    pub const fn generated(mut self, snapshot: &'static str, dir: &'static str) -> Self {
        self.generated = Some(Generated { snapshot, dir });
        self
    }

    /// It with the companion tables of its models: each model's
    /// [`translation_schema`](super::ModelSchema::translation_schema), or `None` for a model with
    /// no `Localized` text.
    #[must_use]
    pub const fn companions(mut self, companions: &'static [fn() -> Option<TableSchema>]) -> Self {
        self.companions = companions;
        self
    }

    /// The companion tables of its models, as [`companions`](Self::companions) named them.
    #[must_use]
    pub const fn companion_tables(&self) -> &'static [fn() -> Option<TableSchema>] {
        self.companions
    }

    /// Every table its models declare: each model's own table, then each companion table.
    #[must_use]
    pub fn schemas(&self) -> Vec<TableSchema> {
        self.tables
            .iter()
            .map(|table| table())
            .chain(self.companions.iter().filter_map(|companion| companion()))
            .collect()
    }

    /// It with `tables` retired: tables its migrations made that its models no longer hold, which
    /// the next generated file drops.
    #[must_use]
    pub const fn retired(mut self, tables: &'static [&'static str]) -> Self {
        self.retired = tables;
        self
    }

    /// The tables it retired.
    #[must_use]
    pub const fn retired_tables(&self) -> &'static [&'static str] {
        self.retired
    }

    /// It with `files` making a role without looking it up in `pg_roles` first, which a migrator
    /// runs only with `CREATEROLE`; [`check`](super::generate::check) refuses any other such file.
    #[must_use]
    pub const fn needing_createrole(mut self, files: &'static [&'static str]) -> Self {
        self.needs_createrole = files;
        self
    }

    /// The files it says a migrator runs only with `CREATEROLE`.
    #[must_use]
    pub const fn files_needing_createrole(&self) -> &'static [&'static str] {
        self.needs_createrole
    }

    /// It with `files` dropping or renaming by hand what no model retires or says `was`;
    /// [`check`](super::generate::check) refuses a file named here that drops nothing.
    #[must_use]
    pub const fn dropping_by_hand(mut self, files: &'static [&'static str]) -> Self {
        self.drops_by_hand = files;
        self
    }

    /// The files it says drop or rename by hand.
    #[must_use]
    pub const fn files_dropping_by_hand(&self) -> &'static [&'static str] {
        self.drops_by_hand
    }

    /// Where the generator keeps what it wrote, when the set is generated.
    #[must_use]
    pub const fn generation(&self) -> Option<Generated> {
        self.generated
    }

    /// A set for the CQL wire (YCQL), owned by `owner`, applied in the order given; recorded
    /// and checked as [`sql`](Self::sql) sets are, in the keyspace's own ledger.
    #[must_use]
    pub const fn cql(
        owner: &'static str,
        migrations: &'static [EmbeddedMigration],
        tables: &'static [fn() -> TableSchema],
    ) -> Self {
        Self {
            owner,
            database: MigrationDatabase::CqlDb,
            migrations,
            tables,
            companions: &[],
            generated: None,
            retired: &[],
            needs_createrole: &[],
            drops_by_hand: &[],
        }
    }

    /// The crate that owns these tables.
    #[must_use]
    pub const fn owner(&self) -> &'static str {
        self.owner
    }

    /// The engine the migrations are written for.
    #[must_use]
    pub const fn database(&self) -> MigrationDatabase {
        self.database
    }

    /// The migrations, in the order they apply.
    #[must_use]
    pub const fn migrations(&self) -> &'static [EmbeddedMigration] {
        self.migrations
    }

    /// The models the migrations create.
    #[must_use]
    pub const fn tables(&self) -> &'static [fn() -> TableSchema] {
        self.tables
    }

    /// The name `migration` is recorded under in an app's ledger: `<owner>/<name>`.
    #[must_use]
    pub fn ledger_name(&self, migration: &EmbeddedMigration) -> String {
        format!("{}/{}", self.owner, migration.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static MIGRATIONS: [EmbeddedMigration; 1] = [EmbeddedMigration::new(
        "0001_widgets",
        "CREATE TABLE IF NOT EXISTS widgets (id UUID NOT NULL, PRIMARY KEY (id HASH));",
    )];
    static SET: EmbeddedMigrations =
        EmbeddedMigrations::sql("fundamental_widgets", &MIGRATIONS, &[]);

    #[test]
    fn a_migration_is_recorded_under_its_owner() {
        assert_eq!(
            SET.ledger_name(&SET.migrations()[0]),
            "fundamental_widgets/0001_widgets"
        );
        assert_eq!(SET.database(), MigrationDatabase::SqlDb);
    }

    static CQL_MIGRATIONS: [EmbeddedMigration; 1] = [EmbeddedMigration::new(
        "0001_widgets",
        "CREATE TABLE IF NOT EXISTS widgets (id text PRIMARY KEY);",
    )];
    static CQL_SET: EmbeddedMigrations =
        EmbeddedMigrations::cql("fundamental_widgets", &CQL_MIGRATIONS, &[]);

    #[test]
    fn a_cql_set_is_written_for_the_cql_wire() {
        assert_eq!(CQL_SET.database(), MigrationDatabase::CqlDb);
        assert_eq!(
            CQL_SET.ledger_name(&CQL_SET.migrations()[0]),
            "fundamental_widgets/0001_widgets"
        );
    }

    #[test]
    fn a_set_names_the_files_that_drop_by_hand() {
        static NAMED: EmbeddedMigrations = EmbeddedMigrations::sql("portfolio", &MIGRATIONS, &[])
            .dropping_by_hand(&["0001_widgets"]);

        assert_eq!(NAMED.files_dropping_by_hand(), ["0001_widgets"]);
        assert!(SET.files_dropping_by_hand().is_empty());
    }
}
