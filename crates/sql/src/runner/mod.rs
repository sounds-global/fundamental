//! Migration runner for YSQL: one transaction per statement, since DDL in a transaction block
//! is not atomic on this engine, with each step recorded in the ledger.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use fundamental_db::migration::ddl::why_not_rerunnable;
use fundamental_db::migration::{
    CatalogProof, LedgerError, MigrationGuard, MigrationLedger, Resolution, Resolve, Retirements,
    StepRecord, StepState, content_hash, split_statements,
};
use sqlx::Row as _;
use tracing::field::Empty;
use tracing::{Span, debug, info, instrument, warn};

mod step_ledger;

use step_ledger::{Step, StepLedger, Tables, applied_rows, resolution_rows, step_rows};

use crate::SqlError;
use crate::pool::SqlPool;

/// Tracking table for applied migrations.
const TRACKING_TABLE: &str = "_fundamental_migrations";

/// The schema-qualified tracking table on `pool`; `SchemaName` is validated, so interpolating it is safe.
fn tracking(pool: &SqlPool) -> String {
    pool.schema().qualify(TRACKING_TABLE)
}

/// The advisory-lock key for `ledger`, so migrators of one schema serialise and of two do not.
fn ledger_lock_key(ledger: &str) -> i64 {
    let digest = blake3::derive_key("fundamental migration ledger lock", ledger.as_bytes());
    let mut key = [0u8; 8];
    key.copy_from_slice(&digest[..8]);
    i64::from_le_bytes(key)
}

/// Whether the engine's answer to a step was lost; `57014`, a statement timeout, is an answer.
fn answer_lost(error: &SqlError) -> bool {
    error.loses_the_answer() && error.aborting_sqlstate().as_deref() != Some("57014")
}

/// How many times one step is attempted: DDL contends on a cluster-wide catalog latch at every deploy.
pub(super) const MIGRATION_ATTEMPTS: u32 = 24;

/// Applies YSQL migrations in order, exactly once each, held to a [`MigrationGuard`].
#[derive(Debug)]
pub struct SqlMigrator {
    guard: MigrationGuard,
    retirements: Retirements,
}

impl SqlMigrator {
    /// Creates a migrator that holds every migration to `guard`.
    #[must_use]
    pub fn new(guard: MigrationGuard) -> Self {
        Self {
            guard,
            retirements: Retirements::none(),
        }
    }

    /// Passes the drops and renames the owners of embedded migrations declare in `retirements`.
    #[must_use]
    pub fn passing(mut self, retirements: Retirements) -> Self {
        self.retirements = retirements;
        self
    }

    fn judge(&self, migrations: &[(&str, &str)]) -> Result<(), SqlError> {
        self.guard
            .inspect_sql_passing(migrations, &self.retirements)?;
        Ok(())
    }

    /// Returns the names of all applied migrations, in application order; an absent ledger is nothing applied.
    ///
    /// # Errors
    /// Returns a [`SqlError`] if the tracking table exists and cannot be read.
    #[instrument(level = "debug", skip(self, pool), fields(applied = Empty), err(Debug))]
    pub async fn applied(&self, pool: &SqlPool) -> Result<Vec<String>, SqlError> {
        let ledger = tracking(pool);
        // Asked on every lease, so it costs no DDL.
        let rows = read_or_none(
            pool,
            format!("SELECT name FROM {ledger} ORDER BY applied_at, name"),
        )
        .await?;
        let names: Vec<String> = rows
            .iter()
            .map(|row| row.try_get::<String, _>("name"))
            .collect::<Result<_, _>>()?;
        Span::current().record("applied", names.len());
        Ok(names)
    }

    /// Where each of `migrations` stands in `pool`, read without the lock and without creating anything.
    ///
    /// # Errors
    /// Returns a [`SqlError`] if a ledger table exists and cannot be read.
    #[instrument(level = "debug", skip(self, pool, migrations), err(Debug))]
    pub async fn ledger(
        &self,
        pool: &SqlPool,
        migrations: &[(&str, &str)],
    ) -> Result<MigrationLedger, SqlError> {
        let tables = Tables::of(pool);
        let applied: BTreeMap<String, DateTime<Utc>> =
            applied_rows(&read_or_none(pool, tables.select_applied()).await?)?
                .into_iter()
                .map(|(name, (at, _))| (name, at))
                .collect();
        let steps = step_rows(&read_or_none(pool, tables.select_steps()).await?)?;
        let resolutions = resolution_rows(&read_or_none(pool, tables.select_resolutions()).await?)?;
        Ok(MigrationLedger::read(
            migrations,
            &applied,
            &steps,
            &resolutions,
        ))
    }

    /// Applies all pending migrations from `migrations` (`(name, sql)` pairs) in order, one step
    /// per statement; returns the names applied by this call.
    ///
    /// # Errors
    /// Returns [`SqlError::Refused`] if the guard rejects any migration in the set, before any
    /// runs; [`LedgerError::Unresolved`] (as [`SqlError::Ledger`]) while a migration in the set
    /// has an unsettled failed step; [`SqlError::MigrationFailed`] naming the step that failed.
    #[instrument(
        skip(self, pool, migrations),
        fields(candidates = migrations.len(), applied = Empty, skipped = Empty),
        err(Debug)
    )]
    pub async fn apply_all(
        &self,
        pool: &SqlPool,
        migrations: &[(&str, &str)],
    ) -> Result<Vec<String>, SqlError> {
        // Whole-set, up front, so no migration commits before an offending one is found.
        self.judge(migrations)?;

        // Without the lock or DDL first, so a store with nothing pending pays for neither.
        let applied = self.applied_hashes(pool).await?;
        let pending = Self::pending(&applied, migrations)?;
        let mut skipped = migrations.len() - pending.len();

        let newly_applied = if pending.is_empty() {
            Vec::new()
        } else {
            let ledger = StepLedger::open(pool, ledger_lock_key(&tracking(pool))).await?;
            let outcome = self.apply_locked(&ledger, &pending).await;
            ledger.close().await;
            outcome?
        };
        skipped += pending.len() - newly_applied.len();

        let span = Span::current();
        span.record("applied", newly_applied.len());
        span.record("skipped", skipped);
        info!(
            applied = newly_applied.len(),
            skipped, "sql migrations up to date"
        );
        Ok(newly_applied)
    }

    fn pending<'m>(
        applied: &BTreeMap<String, String>,
        migrations: &[(&'m str, &'m str)],
    ) -> Result<Vec<(&'m str, &'m str)>, SqlError> {
        let mut pending = Vec::new();
        for (name, sql) in migrations {
            match applied.get(*name) {
                Some(recorded) => {
                    Self::unchanged(name, recorded, sql)?;
                    debug!(migration = %name, "migration already applied; skipping");
                }
                None => pending.push((*name, *sql)),
            }
        }
        Ok(pending)
    }

    #[expect(
        clippy::cognitive_complexity,
        reason = "one loop; each tracing event's expansion is counted"
    )]
    async fn apply_locked(
        &self,
        ledger: &StepLedger<'_>,
        pending: &[(&str, &str)],
    ) -> Result<Vec<String>, SqlError> {
        ledger.settle_stopped().await?;
        // Again under the lock: another migrator may have applied some meanwhile.
        let applied = ledger.applied().await?;
        let steps = ledger.steps().await?;
        Self::refuse_unresolved(pending, &applied, &steps)?;
        let mut newly_applied = Vec::new();
        for (name, sql) in pending {
            if let Some((_, recorded)) = applied.get(*name) {
                Self::unchanged(name, recorded, sql)?;
                info!(migration = %name, "migration applied by another process while this one waited; skipping");
                continue;
            }
            self.run_steps(ledger, name, sql, &steps).await?;
            info!(migration = %name, "sql migration applied");
            newly_applied.push((*name).to_owned());
        }
        Ok(newly_applied)
    }

    /// Refuses `pending` while one of them has a failed step, rather than repeat the outage.
    fn refuse_unresolved(
        pending: &[(&str, &str)],
        applied: &BTreeMap<String, (DateTime<Utc>, String)>,
        steps: &[StepRecord],
    ) -> Result<(), SqlError> {
        match pending
            .iter()
            .filter(|(name, _)| !applied.contains_key(*name))
            .find_map(|(name, _)| failed_step(steps, name))
        {
            Some(failed) => Err(LedgerError::Unresolved {
                name: failed.name.clone(),
                step: failed.step,
                total: failed.total,
            }
            .into()),
            None => Ok(()),
        }
    }

    #[instrument(skip(self, ledger, sql, steps), fields(migration = name, statements = Empty), err(Debug))]
    async fn run_steps(
        &self,
        ledger: &StepLedger<'_>,
        name: &str,
        sql: &str,
        steps: &[StepRecord],
    ) -> Result<(), SqlError> {
        let statements = split_statements(sql);
        let total = statements.len();
        Span::current().record("statements", total);
        let hash = content_hash(sql);
        let mut last_skipped = total == 0;
        for (index, statement) in statements.iter().enumerate() {
            let step = Step {
                name,
                number: index + 1,
                total,
                statement,
                hash: content_hash(statement),
            };
            let done = steps.iter().any(|record| {
                record.name == name
                    && record.step == step.number
                    && record.state == StepState::Done
                    && record.statement_hash == step.hash
            });
            if done {
                if is_setting(statement) {
                    // A setting lives in the session that made it, and the steps after it rely on it.
                    ledger.reapply(statement).await?;
                }
                debug!(
                    migration = name,
                    step = step.number,
                    "step already done; skipping"
                );
                last_skipped = step.number == total;
                continue;
            }
            ledger.start(&step).await?;
            if let Err(error) = ledger.run(&step, &hash).await {
                if error.is_retryable() {
                    ledger.forget(&step).await;
                } else {
                    ledger.fail(&step, &error, answer_lost(&error)).await;
                }
                warn!(
                    migration = name,
                    step = step.number,
                    total,
                    error = %error,
                    "a migration step failed"
                );
                return Err(Self::failed(&step, error));
            }
        }
        if last_skipped {
            ledger.record_empty(name, &hash).await?;
        }
        Ok(())
    }

    /// Settles the failed step of `migration` by running it and every later step again; returns the step it ran from.
    ///
    /// # Errors
    /// [`LedgerError::NothingToResolve`] when `name` is applied or has no failed step;
    /// [`LedgerError::NotRerunnable`] when the failed step's answer was lost and its statement
    /// cannot run again; [`SqlError::MigrationFailed`] when a step fails again.
    #[instrument(skip(self, pool, sql, resolution), fields(by = resolution.by()), err(Debug))]
    pub async fn retry(
        &self,
        pool: &SqlPool,
        (name, sql): (&str, &str),
        resolution: &Resolution,
    ) -> Result<StepRecord, SqlError> {
        let ledger = StepLedger::open(pool, ledger_lock_key(&tracking(pool))).await?;
        let outcome = self.retry_locked(&ledger, name, sql, resolution).await;
        ledger.close().await;
        outcome
    }

    async fn retry_locked(
        &self,
        ledger: &StepLedger<'_>,
        name: &str,
        sql: &str,
        resolution: &Resolution,
    ) -> Result<StepRecord, SqlError> {
        let (failed, steps) = Self::unsettled(ledger, name).await?;
        if failed.answer_lost {
            let statement = split_statements(sql)
                .get(failed.step.saturating_sub(1))
                .cloned()
                .unwrap_or_default();
            if let Some(why) = why_not_rerunnable(&statement) {
                return Err(LedgerError::NotRerunnable {
                    name: name.to_owned(),
                    step: failed.step,
                    total: failed.total,
                    why: why.to_owned(),
                }
                .into());
            }
        }
        ledger
            .resolve(&failed, Resolve::Retry, resolution, &content_hash(sql))
            .await?;
        info!(
            migration = name,
            step = failed.step,
            "retrying a failed migration"
        );
        self.run_steps(ledger, name, sql, &steps).await?;
        Ok(failed)
    }

    /// Settles the failed step of `migration` by recording it applied, as `proof` shows; returns the step it settled.
    ///
    /// # Errors
    /// [`LedgerError::NothingToResolve`] when `name` is applied or has no failed step.
    #[instrument(skip(self, pool, sql, resolution, proof), fields(by = resolution.by(), effects = proof.effects()), err(Debug))]
    pub async fn mark_applied(
        &self,
        pool: &SqlPool,
        (name, sql): (&str, &str),
        resolution: &Resolution,
        proof: &CatalogProof,
    ) -> Result<StepRecord, SqlError> {
        let ledger = StepLedger::open(pool, ledger_lock_key(&tracking(pool))).await?;
        let outcome = async {
            let (failed, _) = Self::unsettled(&ledger, name).await?;
            ledger
                .resolve(&failed, Resolve::Applied, resolution, &content_hash(sql))
                .await?;
            info!(
                migration = name,
                "a failed migration recorded as applied, its catalog proved"
            );
            Ok(failed)
        }
        .await;
        ledger.close().await;
        outcome
    }

    async fn unsettled(
        ledger: &StepLedger<'_>,
        name: &str,
    ) -> Result<(StepRecord, Vec<StepRecord>), SqlError> {
        ledger.settle_stopped().await?;
        let nothing = || LedgerError::NothingToResolve {
            name: name.to_owned(),
        };
        if ledger.applied().await?.contains_key(name) {
            return Err(nothing().into());
        }
        let steps = ledger.steps().await?;
        let failed = failed_step(&steps, name).cloned().ok_or_else(nothing)?;
        Ok((failed, steps))
    }

    fn unchanged(name: &str, recorded: &str, sql: &str) -> Result<(), SqlError> {
        let found = content_hash(sql);
        if recorded == found {
            return Ok(());
        }
        Err(SqlError::MigrationChanged {
            name: name.to_owned(),
            applied: recorded.to_owned(),
            found,
        })
    }

    async fn applied_hashes(&self, pool: &SqlPool) -> Result<BTreeMap<String, String>, SqlError> {
        let tables = Tables::of(pool);
        Ok(
            applied_rows(&read_or_none(pool, tables.select_applied()).await?)?
                .into_iter()
                .map(|(name, (_, hash))| (name, hash))
                .collect(),
        )
    }

    /// Builds a [`SqlError::MigrationFailed`] naming the migration and its step, keeping the engine's error.
    fn failed(step: &Step<'_>, error: SqlError) -> SqlError {
        SqlError::MigrationFailed {
            name: step.name.to_owned(),
            step: step.number,
            total: step.total,
            statement: step.statement.to_owned(),
            cause: error.into(),
        }
    }
}

/// Whether `statement` changes a setting of the session it runs in: `SET` or `RESET`.
fn is_setting(statement: &str) -> bool {
    statement
        .split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("SET") || word.eq_ignore_ascii_case("RESET"))
}

fn failed_step<'s>(steps: &'s [StepRecord], name: &str) -> Option<&'s StepRecord> {
    steps
        .iter()
        .find(|record| record.name == name && record.state == StepState::Failed)
}

/// The rows `sql` reads from a ledger table, or none when it does not exist, retried on the migrator's budget.
///
/// A `dyn Future`, as the concrete one exceeds the compiler's query depth in a migrate binary.
fn read_or_none(
    pool: &SqlPool,
    sql: String,
) -> Pin<Box<dyn Future<Output = Result<Vec<sqlx::postgres::PgRow>, SqlError>> + Send + '_>> {
    Box::pin(async move {
        match pool
            .with_retry_budget(MIGRATION_ATTEMPTS, || async { pool.fetch_all(&sql).await })
            .await
        {
            Ok(rows) => Ok(rows),
            Err(error) if error.is_undefined_table() => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    })
}

#[cfg(test)]
mod tests {
    use fundamental_db::migration::{
        EmbeddedMigrations, Exemption, Lints, MigrationGuard, Retirements, TableSchema,
    };

    use super::SqlMigrator;
    use crate::SqlError;

    const NONE: &[Exemption] = &[];

    fn widgets() -> TableSchema {
        let mut table = TableSchema::new("widgets");
        table.retired.columns = vec!["legacy".to_owned()];
        table
    }

    static ACME: EmbeddedMigrations =
        EmbeddedMigrations::sql("acme", &[], &[widgets as fn() -> TableSchema]);

    const DROP: &[(&str, &str)] = &[("acme/0002_w", "ALTER TABLE widgets DROP COLUMN legacy;")];

    fn migrator() -> SqlMigrator {
        SqlMigrator::new(MigrationGuard::new(1_000, Lints::default(), NONE))
    }

    #[test]
    fn a_set_or_a_reset_is_a_setting_and_an_update_that_sets_is_not() {
        assert!(super::is_setting("SET statement_timeout = 0;"));
        assert!(super::is_setting("reset statement_timeout"));
        assert!(!super::is_setting("UPDATE orders SET note = 'x';"));
        assert!(!super::is_setting(
            "ALTER TABLE orders ALTER COLUMN note SET DEFAULT 'x';"
        ));
    }

    #[test]
    fn a_migrator_passes_what_its_owners_declare() {
        migrator()
            .passing(Retirements::of([&ACME]))
            .judge(DROP)
            .expect("acme retires widgets.legacy");
    }

    #[test]
    fn a_migrator_given_no_declarations_refuses_the_drop() {
        let error = migrator().judge(DROP).expect_err("refused");
        assert!(matches!(error, SqlError::Refused(_)), "{error:?}");
    }
}
