//! The app-resident migration entry point.

use std::collections::BTreeSet;
#[cfg(feature = "sql")]
use std::ptr;

use fundamental_core::model::{CqlDb, Offline, SqlDb};

#[cfg(feature = "cql")]
use fundamental_cql::CqlPool;
#[cfg(feature = "cql")]
use fundamental_cql::migration::CqlIntrospector;
#[cfg(feature = "cql")]
use fundamental_cql::runner::CqlMigrator;
#[cfg(feature = "sql")]
use fundamental_db::migration::{Retirements, split_statements};
#[cfg(feature = "offline")]
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
#[cfg(feature = "offline")]
use fundamental_offline::migration::OfflineIntrospector;
#[cfg(feature = "offline")]
use fundamental_offline::runner::OfflineMigrator;
#[cfg(feature = "sql")]
use fundamental_sql::migration::SqlIntrospector;
#[cfg(feature = "sql")]
use fundamental_sql::runner::SqlMigrator;
#[cfg(feature = "sql")]
use fundamental_sql::{SqlError, SqlPool};

use fundamental_db::DatabaseError;
#[cfg(any(feature = "sql", feature = "cql"))]
use fundamental_db::migration::DiffConfig;
use fundamental_db::migration::{
    DatabaseSchema, EmbeddedMigration, EmbeddedMigrations, MigrationConfig, MigrationDatabase,
    MigrationError, MigrationGuard, MigrationResult, ModelSchema, SchemaDrift, SchemaIntrospector,
    TableSchema,
};
#[cfg(all(feature = "sync", any(feature = "sql", feature = "cql")))]
use fundamental_sync::{Syncable, mirror_table_schema};

use tracing::field::Empty;
use tracing::{Span, instrument};

use crate::MigrationManager;
use crate::migration_check::{DRIFT_BLIND_SPOTS, MigrationCheck};
use crate::migration_path::{is_legal_name, up_file};

/// How many times one replayed baseline statement is attempted.
///
/// DDL on this engine contends on a cluster-wide catalog latch and meets `40001`; every seed is
/// written `ON CONFLICT DO NOTHING`, so a replayed statement may run again.
#[cfg(feature = "sql")]
const REPLAY_ATTEMPTS: u32 = 24;

/// A function that produces a registered model's expected [`TableSchema`].
type SchemaFn = fn() -> TableSchema;

/// A model's companion table, `None` when the model has no `Localized` text.
type CompanionFn = fn() -> Option<TableSchema>;

/// Collects an application's models per backend and drives the migration
/// workflow against a live database.
pub struct MigrateRunner {
    manager: MigrationManager,
    // Read by the server wires only: an on-device store is held to none of the guard's rules.
    #[cfg_attr(
        not(any(feature = "sql", feature = "cql")),
        expect(dead_code, reason = "only a database wire runs it")
    )]
    pub(crate) guard: MigrationGuard,
    sql: Vec<SchemaFn>,
    /// The companion tables of the models, each with its engine: where their `Localized`
    /// text keeps its other languages.
    companions: Vec<(MigrationDatabase, CompanionFn)>,
    cql: Vec<SchemaFn>,
    offline: Vec<SchemaFn>,
    embedded: Vec<&'static EmbeddedMigrations>,
}

impl MigrateRunner {
    /// Creates a runner over `config.migrations_dir`, holding every migration to `guard`.
    pub fn new(config: MigrationConfig, guard: MigrationGuard) -> Self {
        Self {
            manager: MigrationManager::new(config),
            guard,
            sql: Vec::new(),
            companions: Vec::new(),
            cql: Vec::new(),
            offline: Vec::new(),
            embedded: Vec::new(),
        }
    }

    /// Registers the migrations and models a crate ships for the tables it owns.
    ///
    /// Its migrations apply before the app's directory, in registration order, recorded as
    /// `<owner>/<name>`.
    #[must_use]
    pub fn with_embedded(mut self, set: &'static EmbeddedMigrations) -> Self {
        let models = match set.database() {
            MigrationDatabase::SqlDb => &mut self.sql,
            MigrationDatabase::CqlDb => &mut self.cql,
            MigrationDatabase::OfflineDb => &mut self.offline,
        };
        models.extend_from_slice(set.tables());
        self.companions.extend(
            set.companion_tables()
                .iter()
                .map(|companion| (set.database(), *companion)),
        );
        self.embedded.push(set);
        self
    }

    /// Registers a YSQL model.
    #[must_use]
    pub fn sql_model<M: ModelSchema<SqlDb>>(mut self) -> Self {
        self.sql.push(<M as ModelSchema<SqlDb>>::table_schema);
        self.companions.push((
            MigrationDatabase::SqlDb,
            <M as ModelSchema<SqlDb>>::translation_schema,
        ));
        self
    }

    /// Registers a YCQL model, and its companion table when it has `Localized` text.
    #[must_use]
    pub fn cql_model<M: ModelSchema<CqlDb>>(mut self) -> Self {
        self.cql.push(<M as ModelSchema<CqlDb>>::table_schema);
        self.companions.push((
            MigrationDatabase::CqlDb,
            <M as ModelSchema<CqlDb>>::translation_schema,
        ));
        self
    }

    /// Registers an offline (SQLite) model, and its companion table when it has `Localized`
    /// text.
    #[must_use]
    pub fn offline_model<M: ModelSchema<Offline>>(mut self) -> Self {
        self.offline.push(<M as ModelSchema<Offline>>::table_schema);
        self.companions.push((
            MigrationDatabase::OfflineDb,
            <M as ModelSchema<Offline>>::translation_schema,
        ));
        self
    }

    /// Registers a `#[model(syncable)]` model bound to YSQL: its server table and its offline
    /// mirror.
    #[must_use]
    #[cfg(all(feature = "sql", feature = "sync"))]
    pub fn syncable_sql_model<M: ModelSchema<SqlDb> + Syncable>(mut self) -> Self {
        self.sql.push(<M as ModelSchema<SqlDb>>::table_schema);
        self.companions.push((
            MigrationDatabase::SqlDb,
            <M as ModelSchema<SqlDb>>::translation_schema,
        ));
        self.offline.push(mirror_table_schema::<M>);
        self
    }

    /// Registers a `#[model(syncable)]` model bound to YCQL: its server table and its offline
    /// mirror.
    #[must_use]
    #[cfg(all(feature = "cql", feature = "sync"))]
    pub fn syncable_cql_model<M: ModelSchema<CqlDb> + Syncable>(mut self) -> Self {
        self.cql.push(<M as ModelSchema<CqlDb>>::table_schema);
        self.companions.push((
            MigrationDatabase::CqlDb,
            <M as ModelSchema<CqlDb>>::translation_schema,
        ));
        self.offline.push(mirror_table_schema::<M>);
        self
    }

    /// The underlying [`MigrationManager`], for status, safety checks, and file ops.
    pub fn manager(&self) -> &MigrationManager {
        &self.manager
    }

    /// The expected schema for `database`, assembled from its registered models.
    pub fn expected_schema(&self, database: MigrationDatabase) -> DatabaseSchema {
        let models = match database {
            MigrationDatabase::SqlDb => &self.sql,
            MigrationDatabase::CqlDb => &self.cql,
            MigrationDatabase::OfflineDb => &self.offline,
        };
        let mut schema = DatabaseSchema::for_database(database);
        for build in models {
            schema.add_table(build());
        }
        for companion in self
            .companions
            .iter()
            .filter(|(engine, _)| *engine == database)
            .filter_map(|(_, companion)| companion())
        {
            schema.add_table(companion);
        }
        schema
    }

    /// Applies all pending YSQL migrations on disk, in order, recording
    /// each in the `_fundamental_migrations` table. Returns the names applied.
    ///
    /// # Errors
    /// Returns a [`MigrationError`](fundamental_db::migration::MigrationError) if
    /// a migration file cannot be read or a migration fails to apply.
    #[instrument(skip(self, pool), fields(applied = Empty), err(Debug))]
    #[cfg(feature = "sql")]
    pub async fn apply_sql(&self, pool: &SqlPool) -> MigrationResult<Vec<String>> {
        let files = self.pending_files(MigrationDatabase::SqlDb)?;
        let pairs: Vec<(&str, &str)> = files
            .iter()
            .map(|(n, s)| (n.as_str(), s.as_str()))
            .collect();
        let applied = SqlMigrator::new(self.guard.clone())
            .passing(self.retirements())
            .apply_all(pool, &pairs)
            .await
            .map_err(crate::resolve::from_sql)?;
        Span::current().record("applied", applied.len());
        Ok(applied)
    }

    /// What the owners of the registered sets declare they drop and rename, which the guard
    /// passes in their files.
    #[cfg(feature = "sql")]
    pub(crate) fn retirements(&self) -> Retirements {
        Retirements::of(self.embedded.iter().copied())
    }

    /// Applies `files`, each `(ledger name, text)`, as [`apply_sql`](Self::apply_sql) applies the
    /// runner's own: judged whole by the guard first, each one already in the ledger skipped
    /// when its hash agrees.
    #[cfg(feature = "sql")]
    pub(crate) async fn apply_files(
        &self,
        pool: &SqlPool,
        files: &[(String, String)],
    ) -> MigrationResult<Vec<String>> {
        let pairs: Vec<(&str, &str)> = files
            .iter()
            .map(|(n, s)| (n.as_str(), s.as_str()))
            .collect();
        Ok(SqlMigrator::new(self.guard.clone())
            .passing(self.retirements())
            .apply_all(pool, &pairs)
            .await
            .map_err(DatabaseError::from)?)
    }

    /// The registered SQL sets, in the order they apply.
    #[cfg(feature = "sql")]
    pub(crate) fn sql_sets(&self) -> impl Iterator<Item = &'static EmbeddedMigrations> + '_ {
        self.embedded
            .iter()
            .copied()
            .filter(|set| set.database() == MigrationDatabase::SqlDb)
    }

    /// Judges every SQL migration, embedded and on disk, before any of it runs, passing what
    /// each set's owner declares.
    #[cfg(feature = "sql")]
    fn judge_sql(&self) -> MigrationResult<()> {
        let every = self.pending_files(MigrationDatabase::SqlDb)?;
        let every: Vec<(&str, &str)> = every
            .iter()
            .map(|(n, s)| (n.as_str(), s.as_str()))
            .collect();
        self.guard
            .inspect_sql_passing(&every, &self.retirements())
            .map_err(|refused| DatabaseError::from(SqlError::from(refused)))?;
        Ok(())
    }

    /// Applies the pending SQL migrations of every embedded set registered up to and including
    /// `set`, and nothing after it: neither a later set nor the app's directory. Returns the
    /// names applied.
    ///
    /// For a step that must run between a set and the rest.
    ///
    /// # Errors
    /// [`MigrationError::SetNotRegistered`] when `set` was not given to
    /// [`with_embedded`](Self::with_embedded); otherwise as [`apply_sql`](Self::apply_sql).
    #[instrument(skip(self, pool, set), fields(set = set.owner(), applied = Empty), err(Debug))]
    #[cfg(feature = "sql")]
    pub async fn apply_sql_through(
        &self,
        pool: &SqlPool,
        set: &'static EmbeddedMigrations,
    ) -> MigrationResult<Vec<String>> {
        let Some(last) = self
            .embedded
            .iter()
            .position(|registered| ptr::eq(*registered, set))
        else {
            return Err(MigrationError::SetNotRegistered {
                owner: set.owner().to_owned(),
            });
        };
        // Judged whole before the prefix commits, so a refusal leaves nothing applied.
        self.judge_sql()?;
        let through = &self.embedded[..=last];
        let files: Vec<(String, String)> = self
            .embedded_for(MigrationDatabase::SqlDb)?
            .into_iter()
            .filter(|(owner, _)| through.iter().any(|kept| ptr::eq(*kept, *owner)))
            .map(|(owner, migration)| (owner.ledger_name(migration), migration.up().to_owned()))
            .collect();
        let pairs: Vec<(&str, &str)> = files
            .iter()
            .map(|(n, s)| (n.as_str(), s.as_str()))
            .collect();
        let applied = SqlMigrator::new(self.guard.clone())
            .passing(self.retirements())
            .apply_all(pool, &pairs)
            .await
            .map_err(DatabaseError::from)?;
        Span::current().record("applied", applied.len());
        Ok(applied)
    }

    /// Re-applies the **data** statements of every SQL migration, in order, into a schema
    /// whose tables already exist.
    ///
    /// Emptying a pooled schema removes its migrations' baseline rows; this puts them back.
    ///
    /// Returns how many statements were replayed.
    ///
    /// # Errors
    /// Returns a [`MigrationError`](fundamental_db::migration::MigrationError) if a
    /// migration file cannot be read or a statement fails to apply.
    #[instrument(skip(self, pool), fields(replayed = Empty), err(Debug))]
    #[cfg(feature = "sql")]
    pub async fn replay_sql_data(&self, pool: &SqlPool) -> MigrationResult<usize> {
        let files = self.pending_files(MigrationDatabase::SqlDb)?;
        let mut replayed = 0;
        for (_, sql) in &files {
            for statement in split_statements(sql) {
                if !is_data_statement(&statement) {
                    continue;
                }
                pool.with_retry_budget(REPLAY_ATTEMPTS, || async {
                    pool.execute(&statement).await
                })
                .await
                .map_err(DatabaseError::from)?;
                replayed += 1;
            }
        }
        Span::current().record("replayed", replayed);
        Ok(replayed)
    }

    /// Applies all pending YCQL migrations on disk, in order. Returns the
    /// names applied.
    ///
    /// # Errors
    /// Returns a [`MigrationError`](fundamental_db::migration::MigrationError) if
    /// a migration file cannot be read or a migration fails to apply.
    #[instrument(skip(self, pool), fields(applied = Empty), err(Debug))]
    #[cfg(feature = "cql")]
    pub async fn apply_cql(&self, pool: &CqlPool) -> MigrationResult<Vec<String>> {
        let files = self.pending_files(MigrationDatabase::CqlDb)?;
        let pairs: Vec<(&str, &str)> = files
            .iter()
            .map(|(n, s)| (n.as_str(), s.as_str()))
            .collect();
        let applied = CqlMigrator::new(self.guard.clone())
            .apply_all(pool, &pairs)
            .await
            .map_err(crate::resolve::from_cql)?;
        Span::current().record("applied", applied.len());
        Ok(applied)
    }

    /// Applies all pending offline (SQLite) migrations on disk, in order. Returns
    /// the names applied.
    ///
    /// # Errors
    /// Returns a [`MigrationError`](fundamental_db::migration::MigrationError) if
    /// a migration file cannot be read or a migration fails to apply.
    #[instrument(skip(self, db), fields(applied = Empty), err(Debug))]
    #[cfg(feature = "offline")]
    pub async fn apply_offline(&self, db: &OfflineDatabase) -> MigrationResult<Vec<String>> {
        let files = self.pending_files(MigrationDatabase::OfflineDb)?;
        let pairs: Vec<(&str, &str)> = files
            .iter()
            .map(|(n, s)| (n.as_str(), s.as_str()))
            .collect();
        let applied = OfflineMigrator::new()
            .apply_all(db, &pairs)
            .map_err(DatabaseError::from)?;
        Span::current().record("applied", applied.len());
        Ok(applied)
    }

    /// Returns the names of applied YSQL migrations, in application order.
    ///
    /// # Errors
    /// Returns a [`MigrationError`](fundamental_db::migration::MigrationError) if
    /// the tracking table cannot be read.
    #[cfg(feature = "sql")]
    pub async fn applied_sql(&self, pool: &SqlPool) -> MigrationResult<Vec<String>> {
        Ok(SqlMigrator::new(self.guard.clone())
            .applied(pool)
            .await
            .map_err(DatabaseError::from)?)
    }

    /// Returns the migrations on disk that `pool` has no record of applying, in order.
    ///
    /// Reads only the ledger: empty does not mean the tables those migrations made are still there.
    ///
    /// # Errors
    /// Returns a [`MigrationError`](fundamental_db::migration::MigrationError) if the
    /// tracking table cannot be read or the migration directory cannot be listed.
    #[cfg(feature = "sql")]
    pub async fn unapplied_sql(&self, pool: &SqlPool) -> MigrationResult<Vec<String>> {
        self.unapplied(MigrationDatabase::SqlDb, self.applied_sql(pool).await?)
    }

    /// Returns the names of applied YCQL migrations.
    ///
    /// # Errors
    /// Returns a [`MigrationError`](fundamental_db::migration::MigrationError) if
    /// the tracking table cannot be read.
    #[cfg(feature = "cql")]
    pub async fn applied_cql(&self, pool: &CqlPool) -> MigrationResult<Vec<String>> {
        Ok(CqlMigrator::new(self.guard.clone())
            .applied(pool)
            .await
            .map_err(DatabaseError::from)?)
    }

    /// Returns the names of applied offline (SQLite) migrations.
    ///
    /// # Errors
    /// Returns a [`MigrationError`](fundamental_db::migration::MigrationError) if
    /// the tracking table cannot be read.
    #[cfg(feature = "offline")]
    pub async fn applied_offline(&self, db: &OfflineDatabase) -> MigrationResult<Vec<String>> {
        Ok(OfflineMigrator::new()
            .applied(db)
            .map_err(DatabaseError::from)?)
    }

    /// Asserts the live YSQL schema already matches the registered models.
    ///
    /// # Errors
    /// Returns [`MigrationError::SchemaVerification`] listing the drift if the
    /// database does not match the models, or a connection/introspection error.
    #[instrument(skip(self, pool), err(Debug))]
    #[cfg(feature = "sql")]
    pub async fn assert_no_pending_sql(&self, pool: &SqlPool) -> MigrationResult<()> {
        let actual = SqlIntrospector::introspect(pool).await?;
        let expected = self.expected_schema(MigrationDatabase::SqlDb);
        let drifts = self.manager.verify_schema(&expected, &actual)?;
        Self::require_no_drift(drifts)
    }

    /// Checks a SQL database against the migrations and models on disk.
    ///
    /// It refuses on unapplied migrations and only reports schema drift.
    ///
    /// # Errors
    /// Returns [`MigrationError::UnappliedMigrations`] when a migration on disk has not
    /// been applied, or a connection/introspection error. Drift alone is never an error.
    #[instrument(skip(self, pool), err(Debug))]
    #[cfg(feature = "sql")]
    pub async fn check_sql(&self, pool: &SqlPool) -> MigrationResult<MigrationCheck> {
        let unapplied = self.unapplied_sql(pool).await?;
        let actual = SqlIntrospector::introspect(pool).await?;
        let expected = self.expected_schema(MigrationDatabase::SqlDb);
        // A column a later migration adds is appended, and the engine never reorders: the
        // order is not the model's to say, and rows are read by name.
        let config = DiffConfig {
            ignore_column_order: true,
            ..Default::default()
        };
        let drift = self
            .manager
            .verify_schema_with_config(&expected, &actual, config)?;
        Self::report(unapplied, drift)
    }

    /// Asserts the live YCQL schema already matches the registered models.
    ///
    /// # Errors
    /// Returns [`MigrationError::SchemaVerification`] listing the drift if the
    /// keyspace does not match the models, or a connection/introspection error.
    #[instrument(skip(self, pool), err(Debug))]
    #[cfg(feature = "cql")]
    pub async fn assert_no_pending_cql(&self, pool: &CqlPool) -> MigrationResult<()> {
        let actual = CqlIntrospector::introspect(pool).await?;
        let expected = self.expected_schema(MigrationDatabase::CqlDb);
        let config = DiffConfig {
            ignore_column_order: true,
            ..Default::default()
        };
        let drifts = self
            .manager
            .verify_schema_with_config(&expected, &actual, config)?;
        Self::require_no_drift(drifts)
    }

    /// Checks a CQL keyspace against the migrations and models on disk.
    ///
    /// The CQL half of [`check_sql`](Self::check_sql); the same two-speed contract applies.
    ///
    /// # Errors
    /// Returns [`MigrationError::UnappliedMigrations`] when a migration on disk has not
    /// been applied, or a connection/introspection error. Drift alone is never an error.
    #[instrument(skip(self, pool), err(Debug))]
    #[cfg(feature = "cql")]
    pub async fn check_cql(&self, pool: &CqlPool) -> MigrationResult<MigrationCheck> {
        let unapplied = self.unapplied(MigrationDatabase::CqlDb, self.applied_cql(pool).await?)?;
        let actual = CqlIntrospector::introspect(pool).await?;
        let expected = self.expected_schema(MigrationDatabase::CqlDb);
        let config = DiffConfig {
            ignore_column_order: true,
            ..Default::default()
        };
        let drift = self
            .manager
            .verify_schema_with_config(&expected, &actual, config)?;
        Self::report(unapplied, drift)
    }

    /// Checks an offline (SQLite) database against the migrations and models on disk.
    ///
    /// The offline half of [`check_sql`](Self::check_sql).
    ///
    /// # Errors
    /// Returns [`MigrationError::UnappliedMigrations`] when a migration on disk has not
    /// been applied, or an introspection error. Drift alone is never an error.
    #[instrument(skip(self, db), err(Debug))]
    #[cfg(feature = "offline")]
    pub async fn check_offline(&self, db: &OfflineDatabase) -> MigrationResult<MigrationCheck> {
        let unapplied = self.unapplied(
            MigrationDatabase::OfflineDb,
            self.applied_offline(db).await?,
        )?;
        let actual = OfflineIntrospector::introspect(db).await?;
        let expected = self.expected_schema(MigrationDatabase::OfflineDb);
        let drift = self.manager.verify_schema(&expected, &actual)?;
        Self::report(unapplied, drift)
    }

    /// The migrations for `database` — embedded, then on disk — that `applied` has no
    /// record of, in application order.
    fn unapplied(
        &self,
        database: MigrationDatabase,
        applied: Vec<String>,
    ) -> MigrationResult<Vec<String>> {
        let applied: BTreeSet<String> = applied.into_iter().collect();
        Ok(self
            .migration_names(database)?
            .into_iter()
            .filter(|name| !applied.contains(name))
            .collect())
    }

    /// The names every migration for `database` is recorded under, in application order —
    /// what [`pending_files`](Self::pending_files) reads, without reading it.
    fn migration_names(&self, database: MigrationDatabase) -> MigrationResult<Vec<String>> {
        let mut names = Vec::new();
        for (set, migration) in self.embedded_for(database)? {
            names.push(set.ledger_name(migration));
        }
        names.extend(self.manager.list_migrations(database)?);
        Ok(names)
    }

    /// Every embedded migration for `database`, with its set, in application order — each
    /// checked to be nameable, so an owner or name holding a separator is refused before it
    /// reaches a ledger.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) fn embedded_for(
        &self,
        database: MigrationDatabase,
    ) -> MigrationResult<Vec<(&'static EmbeddedMigrations, &'static EmbeddedMigration)>> {
        let mut found = Vec::new();
        for set in self
            .embedded
            .iter()
            .filter(|set| set.database() == database)
        {
            for migration in set.migrations() {
                if !is_legal_name(set.owner()) || !is_legal_name(migration.name()) {
                    return Err(MigrationError::UnsafeName {
                        name: set.ledger_name(migration),
                    });
                }
                found.push((*set, migration));
            }
        }
        Ok(found)
    }

    /// Assembles the report, and refuses on the half of it that is exact.
    fn report(unapplied: Vec<String>, drift: Vec<SchemaDrift>) -> MigrationResult<MigrationCheck> {
        let check = MigrationCheck {
            unapplied,
            drift,
            blind_spots: DRIFT_BLIND_SPOTS,
        };
        if check.has_unapplied() {
            return Err(MigrationError::UnappliedMigrations {
                message: check.summary(),
                names: check.unapplied,
            });
        }
        Ok(check)
    }

    /// Asserts the live offline (SQLite) schema already matches the registered models.
    ///
    /// # Errors
    /// Returns [`MigrationError::SchemaVerification`] listing the drift if the
    /// database does not match the models, or an introspection error.
    #[instrument(skip(self, db), err(Debug))]
    #[cfg(feature = "offline")]
    pub async fn assert_no_pending_offline(&self, db: &OfflineDatabase) -> MigrationResult<()> {
        let actual = OfflineIntrospector::introspect(db).await?;
        let expected = self.expected_schema(MigrationDatabase::OfflineDb);
        let drifts = self.manager.verify_schema(&expected, &actual)?;
        Self::require_no_drift(drifts)
    }

    /// Asserts the on-disk offline migrations, replayed into a fresh in-memory database, reproduce
    /// the schema the offline models expect; no live database is needed.
    ///
    /// # Errors
    /// Returns [`MigrationError::SchemaVerification`] if the replayed migrations
    /// do not reproduce the models' schema, or an error applying them.
    #[instrument(skip(self), err(Debug))]
    #[cfg(feature = "offline")]
    pub async fn assert_offline_schema_matches_migrations(&self) -> MigrationResult<()> {
        let db = OfflineDatabase::open(OfflineConfig::memory())
            .await
            .map_err(DatabaseError::from)?;
        self.apply_offline(&db).await?;
        self.assert_no_pending_offline(&db).await
    }

    /// Maps a non-empty drift list to a [`MigrationError::SchemaVerification`].
    fn require_no_drift(drifts: Vec<SchemaDrift>) -> MigrationResult<()> {
        if drifts.is_empty() {
            return Ok(());
        }
        Err(MigrationError::SchemaVerification {
            message: format!(
                "{} schema difference(s) between the models and the live database; \
                 write the migration that closes them (`fun migrate create <name>`)",
                drifts.len()
            ),
            drifts,
        })
    }

    /// Every migration for `database`: the embedded sets in registration order, then each
    /// `<name>.up.<ext>` on disk, sorted by name (the timestamp prefix gives application
    /// order).
    pub(crate) fn pending_files(
        &self,
        database: MigrationDatabase,
    ) -> MigrationResult<Vec<(String, String)>> {
        let mut files: Vec<(String, String)> = self
            .embedded_for(database)?
            .into_iter()
            .map(|(set, migration)| (set.ledger_name(migration), migration.up().to_owned()))
            .collect();
        files.extend(self.disk_files(database)?);
        Ok(files)
    }

    /// Each `<name>.up.<ext>` in the app's own directory for `database`, sorted by name, with
    /// its text.
    pub(crate) fn disk_files(
        &self,
        database: MigrationDatabase,
    ) -> MigrationResult<Vec<(String, String)>> {
        let mut files = Vec::new();
        let dir = self.manager.migration_directory(database);
        let ext = match database {
            MigrationDatabase::SqlDb | MigrationDatabase::OfflineDb => "sql",
            MigrationDatabase::CqlDb => "cql",
        };
        let names = self.manager.list_migrations(database)?;
        files.reserve(names.len());
        for name in names {
            let path = up_file(&dir, &name, ext)?;
            let sql = std::fs::read_to_string(&path)?;
            files.push((name, sql));
        }
        Ok(files)
    }
}

/// Whether `statement` writes rows rather than shaping the schema.
///
/// Asked of the first word: a scan for `INSERT` anywhere would match one in a comment or a `CHECK`.
#[cfg(feature = "sql")]
fn is_data_statement(statement: &str) -> bool {
    let first = statement
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("--"))
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(first.as_str(), "INSERT" | "UPDATE" | "DELETE" | "COPY")
}

#[cfg(all(test, feature = "sql"))]
mod tests {
    use fundamental_db::migration::{
        EmbeddedMigration, EmbeddedMigrations, Lints, MigrationConfig, MigrationGuard, TableSchema,
    };

    use super::MigrateRunner;

    fn widgets() -> TableSchema {
        let mut table = TableSchema::new("widgets");
        table.retired.columns = vec!["legacy".to_owned()];
        table
    }

    static FILES: [EmbeddedMigration; 1] = [EmbeddedMigration::new(
        "0002_widgets_legacy",
        "ALTER TABLE widgets DROP COLUMN IF EXISTS legacy;",
    )];
    static DECLARED: EmbeddedMigrations =
        EmbeddedMigrations::sql("acme", &FILES, &[widgets as fn() -> TableSchema]);
    static UNDECLARED: EmbeddedMigrations = EmbeddedMigrations::sql("acme", &FILES, &[]);

    fn runner(dir: &tempfile::TempDir, set: &'static EmbeddedMigrations) -> MigrateRunner {
        MigrateRunner::new(
            MigrationConfig {
                migrations_dir: dir.path().to_path_buf(),
                environment: "test".to_owned(),
                is_protected: false,
            },
            MigrationGuard::new(1_000, Lints::default(), &[]),
        )
        .with_embedded(set)
    }

    #[test]
    fn a_runner_passes_the_drops_its_sets_declare() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        runner(&dir, &DECLARED)
            .judge_sql()
            .expect("acme retires widgets.legacy");
    }

    #[test]
    fn a_runner_refuses_a_drop_no_set_declares() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        runner(&dir, &UNDECLARED)
            .judge_sql()
            .expect_err("no model retires widgets.legacy");
    }
}
