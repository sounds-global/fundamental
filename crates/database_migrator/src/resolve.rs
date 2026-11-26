//! [`MigrateRunner::status_sql`] and [`MigrateRunner::resolve_sql`], and their YCQL twins:
//! where each migration stands, and the one recorded act that settles a failed one.

#[cfg(feature = "cql")]
use fundamental_cql::migration::CqlIntrospector;
#[cfg(feature = "cql")]
use fundamental_cql::runner::CqlMigrator;
#[cfg(feature = "cql")]
use fundamental_cql::{CqlError, CqlPool};
use fundamental_db::DatabaseError;
#[cfg(feature = "cql")]
use fundamental_db::migration::SchemaIntrospector as _;
#[cfg(feature = "sql")]
use fundamental_db::migration::ddl::{DdlStatement, recognise};
#[cfg(feature = "cql")]
use fundamental_db::migration::generate::Snapshot;
use fundamental_db::migration::{
    CatalogProof, LedgerError, LedgerState, MigrationDatabase, MigrationError, MigrationLedger,
    MigrationResult, Resolution, Resolve, StepRecord, StepReplay, split_statements,
};
#[cfg(feature = "sql")]
use fundamental_sql::migration::SqlIntrospector;
#[cfg(feature = "sql")]
use fundamental_sql::runner::SqlMigrator;
#[cfg(feature = "sql")]
use fundamental_sql::{SqlError, SqlPool};
use tracing::{info, instrument};

use crate::MigrateRunner;
#[cfg(feature = "cql")]
use crate::scratch_keyspace::ScratchKeyspace;
#[cfg(feature = "sql")]
use crate::scratch_schema::ScratchSchema;
#[cfg(feature = "sql")]
use crate::verify::catalog;

impl MigrateRunner {
    /// Where every SQL migration — embedded, then on disk — stands in `pool`: applied with its
    /// time and the resolution that settled it, failed at step n of m with the statement and
    /// the engine's answer, running, pending, or blocked behind a failed one.
    ///
    /// # Errors
    /// A migration file cannot be read, or a ledger table cannot be.
    #[cfg(feature = "sql")]
    #[instrument(skip(self, pool), err(Debug))]
    pub async fn status_sql(&self, pool: &SqlPool) -> MigrationResult<MigrationLedger> {
        let files = self.pending_files(MigrationDatabase::SqlDb)?;
        SqlMigrator::new(self.guard.clone())
            .ledger(pool, &pairs(&files))
            .await
            .map_err(from_sql)
    }

    /// Settles the failed SQL migration `name` by `act`, recording `resolution`. Returns the
    /// step it settled.
    ///
    /// [`Resolve::Retry`] runs the failed step and every step after it again. [`Resolve::Applied`]
    /// records it applied once a replay into an empty schema proves `pool`'s catalog holds its
    /// effect; an index whose build did not finish counts as missing.
    ///
    /// # Errors
    /// [`MigrationError::NotFound`] when no migration is named `name`;
    /// [`MigrationError::Ledger`] with [`LedgerError::NothingToResolve`],
    /// [`LedgerError::NotRerunnable`], [`LedgerError::NotInCatalog`] naming each missing effect,
    /// or [`LedgerError::NoCatalogEffect`]; or what a retried step failed with.
    ///
    /// [`LedgerError::NothingToResolve`]: fundamental_db::migration::LedgerError::NothingToResolve
    /// [`LedgerError::NotRerunnable`]: fundamental_db::migration::LedgerError::NotRerunnable
    /// [`LedgerError::NotInCatalog`]: fundamental_db::migration::LedgerError::NotInCatalog
    /// [`LedgerError::NoCatalogEffect`]: fundamental_db::migration::LedgerError::NoCatalogEffect
    #[cfg(feature = "sql")]
    #[instrument(skip(self, pool, resolution), fields(by = resolution.by()), err(Debug))]
    pub async fn resolve_sql(
        &self,
        pool: &SqlPool,
        name: &str,
        act: Resolve,
        resolution: &Resolution,
    ) -> MigrationResult<StepRecord> {
        let files = self.pending_files(MigrationDatabase::SqlDb)?;
        let position = position(&files, name)?;
        let migration = (name, files[position].1.as_str());
        let migrator = SqlMigrator::new(self.guard.clone()).passing(self.retirements());
        let settled = match act {
            Resolve::Retry => {
                let failed = failed_at(&self.status_sql(pool).await?, name)?;
                let unusable = SqlIntrospector::unusable_indexes(pool)
                    .await
                    .map_err(DatabaseError::from)?;
                no_unfinished_index(name, migration.1, failed, &unusable)?;
                migrator.retry(pool, migration, resolution).await
            }
            Resolve::Applied => {
                let failed = failed_at(&self.status_sql(pool).await?, name)?;
                let proof = Box::pin(self.prove_sql(pool, &files[..=position], failed)).await?;
                migrator
                    .mark_applied(pool, migration, resolution, &proof)
                    .await
            }
        }
        .map_err(from_sql)?;
        info!(migration = name, %act, step = settled.step, "a failed sql migration settled");
        Ok(settled)
    }

    /// That `pool`'s catalog holds the effect of the last of `files`, which failed at step
    /// `failed`: the files before it replayed into an empty schema, then each of its steps.
    #[cfg(feature = "sql")]
    async fn prove_sql(
        &self,
        pool: &SqlPool,
        files: &[(String, String)],
        failed: usize,
    ) -> MigrationResult<CatalogProof> {
        let Some(((name, text), before_it)) = files.split_last() else {
            return Err(MigrationError::NotFound(String::new()));
        };
        let scratch = ScratchSchema::create(pool, "resolve").await?;
        let replayed = async {
            self.apply_files(scratch.pool(), before_it).await?;
            let mut replay = StepReplay::new(catalog(scratch.pool()).await?.0);
            for (step, statement) in split_statements(text).into_iter().enumerate() {
                let alone = (format!("{name}.step{}", step + 1), statement.clone());
                self.apply_files(scratch.pool(), std::slice::from_ref(&alone))
                    .await?;
                replay.step(statement, catalog(scratch.pool()).await?.0);
            }
            MigrationResult::Ok(replay)
        }
        .await;
        scratch.remove(pool).await;
        let replay = replayed?;
        let (live, _) = catalog(pool).await?;
        let unusable = SqlIntrospector::unusable_indexes(pool)
            .await
            .map_err(DatabaseError::from)?;
        Ok(CatalogProof::check(
            name, &replay, failed, &live, &unusable,
        )?)
    }

    /// Where every CQL migration on disk stands in `pool`'s keyspace, as
    /// [`status_sql`](Self::status_sql) says it of the SQL ones.
    ///
    /// # Errors
    /// A migration file cannot be read, or a ledger table cannot be.
    #[cfg(feature = "cql")]
    #[instrument(skip(self, pool), err(Debug))]
    pub async fn status_cql(&self, pool: &CqlPool) -> MigrationResult<MigrationLedger> {
        let files = self.pending_files(MigrationDatabase::CqlDb)?;
        CqlMigrator::new(self.guard.clone())
            .ledger(pool, &pairs(&files))
            .await
            .map_err(from_cql)
    }

    /// Settles the failed CQL migration `name` by `act`, recording `resolution`, as
    /// [`resolve_sql`](Self::resolve_sql) does for SQL: the replay for [`Resolve::Applied`]
    /// runs in an empty keyspace beside `pool`'s.
    ///
    /// # Errors
    /// As [`resolve_sql`](Self::resolve_sql).
    #[cfg(feature = "cql")]
    #[instrument(skip(self, pool, resolution), fields(by = resolution.by()), err(Debug))]
    pub async fn resolve_cql(
        &self,
        pool: &CqlPool,
        name: &str,
        act: Resolve,
        resolution: &Resolution,
    ) -> MigrationResult<StepRecord> {
        let files = self.pending_files(MigrationDatabase::CqlDb)?;
        let position = position(&files, name)?;
        let migration = (name, files[position].1.as_str());
        let migrator = CqlMigrator::new(self.guard.clone());
        let settled = match act {
            Resolve::Retry => migrator.retry(pool, migration, resolution).await,
            Resolve::Applied => {
                let failed = failed_at(&self.status_cql(pool).await?, name)?;
                let proof = Box::pin(self.prove_cql(pool, &files[..=position], failed)).await?;
                migrator
                    .mark_applied(pool, migration, resolution, &proof)
                    .await
            }
        }
        .map_err(from_cql)?;
        info!(migration = name, %act, step = settled.step, "a failed cql migration settled");
        Ok(settled)
    }

    /// That `pool`'s keyspace holds the effect of the last of `files`, which failed at step
    /// `failed`, as [`prove_sql`](Self::prove_sql) proves it of a schema.
    #[cfg(feature = "cql")]
    async fn prove_cql(
        &self,
        pool: &CqlPool,
        files: &[(String, String)],
        failed: usize,
    ) -> MigrationResult<CatalogProof> {
        let Some(((name, text), before_it)) = files.split_last() else {
            return Err(MigrationError::NotFound(String::new()));
        };
        let migrator = CqlMigrator::new(self.guard.clone());
        let scratch = ScratchKeyspace::create(pool).await?;
        let replayed = async {
            migrator
                .apply_all(scratch.pool(), &pairs(before_it))
                .await
                .map_err(from_cql)?;
            let mut replay = StepReplay::new(cql_catalog(scratch.pool()).await?);
            for (step, statement) in split_statements(text).into_iter().enumerate() {
                let alone = format!("{name}.step{}", step + 1);
                migrator
                    .apply_all(scratch.pool(), &[(alone.as_str(), statement.as_str())])
                    .await
                    .map_err(from_cql)?;
                replay.step(statement, cql_catalog(scratch.pool()).await?);
            }
            MigrationResult::Ok(replay)
        }
        .await;
        scratch.remove(pool).await;
        let replay = replayed?;
        let live = cql_catalog(pool).await?;
        Ok(CatalogProof::check(name, &replay, failed, &live, &[])?)
    }
}

/// The keyspace `pool` is pinned to, read back as a snapshot.
#[cfg(feature = "cql")]
async fn cql_catalog(pool: &CqlPool) -> MigrationResult<Snapshot> {
    let schema = CqlIntrospector::introspect(pool).await?;
    Ok(Snapshot::of(
        &schema.tables.into_values().collect::<Vec<_>>(),
        None,
    ))
}

/// The step `name` failed at, a step left running included, or [`LedgerError::NothingToResolve`].
fn failed_at(ledger: &MigrationLedger, name: &str) -> MigrationResult<usize> {
    match ledger
        .entries()
        .iter()
        .find(|entry| entry.name() == name)
        .map(fundamental_db::migration::LedgerEntry::state)
    {
        Some(LedgerState::Failed(failed)) => Ok(failed.step),
        Some(LedgerState::Running { step, .. }) => Ok(*step),
        _ => Err(LedgerError::NothingToResolve {
            name: name.to_owned(),
        }
        .into()),
    }
}

/// Refuses a retry of `name` (its text `text`) from step `failed` while one of those steps
/// builds an index `unusable` names: `CREATE INDEX IF NOT EXISTS` would pass over the
/// unfinished one, and the migration would be recorded applied with an index no query uses.
#[cfg(feature = "sql")]
fn no_unfinished_index(
    name: &str,
    text: &str,
    failed: usize,
    unusable: &[(String, String)],
) -> MigrationResult<()> {
    for (index, statement) in split_statements(text)
        .iter()
        .enumerate()
        .skip(failed.saturating_sub(1))
    {
        if let Ok(DdlStatement::CreateIndex {
            name: built, table, ..
        }) = recognise(statement)
            && unusable.iter().any(|(t, i)| *t == table && *i == built)
        {
            return Err(LedgerError::UnfinishedIndex {
                name: name.to_owned(),
                step: index + 1,
                index: built,
                table,
            }
            .into());
        }
    }
    Ok(())
}

/// `files` as the `(name, text)` pairs a migrator takes.
fn pairs(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect()
}

/// Where in `files` the migration `name` is, or [`MigrationError::NotFound`].
fn position(files: &[(String, String)], name: &str) -> MigrationResult<usize> {
    files
        .iter()
        .position(|(file, _)| file == name)
        .ok_or_else(|| MigrationError::NotFound(name.to_owned()))
}

/// A SQL migrator's failure as a migration error: a ledger refusal as itself, so a caller can
/// match it, and anything else through the database layer with its code kept.
#[cfg(feature = "sql")]
pub(crate) fn from_sql(error: SqlError) -> MigrationError {
    match error {
        SqlError::Ledger(refused) => refused.into(),
        other => DatabaseError::from(other).into(),
    }
}

/// [`from_sql`]'s YCQL twin.
#[cfg(feature = "cql")]
pub(crate) fn from_cql(error: CqlError) -> MigrationError {
    match error {
        CqlError::Ledger(refused) => refused.into(),
        other => DatabaseError::from(other).into(),
    }
}

#[cfg(test)]
mod tests {
    use fundamental_db::migration::MigrationError;

    use std::collections::BTreeMap;

    use chrono::Utc;
    use fundamental_db::migration::{LedgerError, MigrationLedger, StepRecord, StepState};

    #[cfg(feature = "sql")]
    use super::no_unfinished_index;
    use super::{failed_at, position};

    fn ledger(state: StepState) -> MigrationLedger {
        let step = StepRecord {
            name: "0002_index_orders".to_owned(),
            step: 2,
            total: 3,
            statement_hash: String::new(),
            state,
            code: None,
            error: None,
            answer_lost: false,
            at: Utc::now(),
        };
        MigrationLedger::read(
            &[
                ("0001_orders", "SELECT 1;"),
                ("0002_index_orders", "SELECT 1; SELECT 2; SELECT 3;"),
            ],
            &BTreeMap::from([("0001_orders".to_owned(), Utc::now())]),
            &[step],
            &[],
        )
    }

    #[test]
    fn the_step_a_migration_failed_or_stopped_at_is_found_before_any_replay() {
        assert_eq!(
            failed_at(&ledger(StepState::Failed), "0002_index_orders").expect("failed"),
            2
        );
        assert_eq!(
            failed_at(&ledger(StepState::Running), "0002_index_orders").expect("stopped"),
            2
        );
        for name in ["0001_orders", "0003_absent"] {
            assert!(
                matches!(
                    failed_at(&ledger(StepState::Failed), name),
                    Err(MigrationError::Ledger(LedgerError::NothingToResolve { .. }))
                ),
                "{name}"
            );
        }
    }

    #[cfg(feature = "sql")]
    #[test]
    fn a_retry_over_an_unfinished_index_it_builds_is_refused() {
        let text = "CREATE INDEX IF NOT EXISTS orders_by_note ON orders (note HASH);\n\
                    SELECT pg_sleep(4);\n\
                    CREATE INDEX IF NOT EXISTS orders_by_customer ON orders (customer_id HASH);";
        let unusable = |index: &str| vec![("orders".to_owned(), index.to_owned())];
        let refused = no_unfinished_index("0002", text, 2, &unusable("orders_by_customer"))
            .expect_err("step 3 would pass over it");
        assert!(
            matches!(
                &refused,
                MigrationError::Ledger(LedgerError::UnfinishedIndex { step: 3, index, table, .. })
                    if index == "orders_by_customer" && table == "orders"
            ),
            "{refused:?}"
        );
        no_unfinished_index("0002", text, 2, &unusable("orders_by_note"))
            .expect("step 1 is done; its index is not this retry's to build");
        no_unfinished_index("0002", text, 2, &[]).expect("nothing unfinished");
    }

    #[test]
    fn a_name_no_file_has_is_not_found() {
        let files = vec![("0001_orders".to_owned(), "SELECT 1;".to_owned())];
        assert_eq!(position(&files, "0001_orders").expect("found"), 0);
        assert!(matches!(
            position(&files, "0002_index_orders"),
            Err(MigrationError::NotFound(name)) if name == "0002_index_orders"
        ));
    }
}
