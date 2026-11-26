//! Migration runner for YCQL.
//!
//! Applies hand-written migrations to a [`CqlPool`] one statement per step, recording each
//! step as it starts and as it ends in `_fundamental_migration_steps` and each migration, once
//! every step is done, in `_fundamental_migrations`, in the session's keyspace.
//!
//! A migration runs in no transaction: YCQL transactions never cover DDL, so a failed step
//! leaves earlier ones applied, and later applies are refused until it is settled.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use fundamental_core::Error as _;
use fundamental_db::migration::ddl::why_not_rerunnable;
use fundamental_db::migration::{
    CatalogProof, LedgerError, MigrationGuard, MigrationLedger, RESOLUTIONS_TABLE, Resolution,
    Resolve, STEPS_TABLE, StepRecord, StepState, content_hash, split_statements,
};
use tracing::field::Empty;
use tracing::{Span, debug, error, info, instrument};

mod migration_lease;
mod step_ledger;

use migration_lease::MigrationLease;
use step_ledger::{StepFailure, StepTables};

use crate::pool::CqlPool;
use crate::{CqlError, CqlIdent, CqlValue, Ddl, cql, ddl};

/// Tracking table for applied migrations.
///
/// CQL rejects unquoted identifiers starting with `_`, so every reference goes through
/// [`tracking`].
const TRACKING_TABLE: &str = "_fundamental_migrations";

/// The keyspace-qualified reference to the tracking table.
///
/// # Errors
/// [`CqlError::InvalidIdentifier`] if `keyspace` is not a plain identifier.
fn tracking(keyspace: &str) -> Result<CqlIdent, CqlError> {
    CqlIdent::qualified(keyspace, TRACKING_TABLE)
}

/// Applies YCQL migrations in order, exactly once each.
///
/// It cannot be built without a [`MigrationGuard`]: CQL has no transaction to roll a bad
/// migration back with. Only the guard's safety rules apply on this wire.
#[derive(Debug)]
pub struct CqlMigrator {
    guard: MigrationGuard,
}

impl CqlMigrator {
    /// Creates a migrator that holds every migration to `guard`.
    #[must_use]
    pub fn new(guard: MigrationGuard) -> Self {
        Self { guard }
    }

    /// Returns the names of all applied migrations.
    ///
    /// # Errors
    /// Returns a [`CqlError`] if the session has no keyspace selected, or the
    /// tracking table cannot be created or read.
    #[instrument(level = "debug", skip(self, pool), fields(applied = Empty), err(Debug))]
    pub async fn applied(&self, pool: &CqlPool) -> Result<Vec<String>, CqlError> {
        let keyspace = self.keyspace(pool)?;
        self.ensure_tracking_table(pool, &keyspace).await?;

        let ledger = tracking(&keyspace)?;
        let mut names: Vec<String> = pool
            .fetch_all::<(String,)>(&cql!("SELECT name FROM {}", ledger))
            .await?
            .into_iter()
            .map(|(name,)| name)
            .collect();
        names.sort();
        Span::current().record("applied", names.len());
        Ok(names)
    }

    /// Where each of `migrations` (`(name, cql)` pairs, in the order they apply) stands in the
    /// session's keyspace: applied, failed at a step, running, pending, or blocked behind a
    /// failure, read without the lease and creating nothing.
    ///
    /// # Errors
    /// A [`CqlError`] if the session is not pinned to a keyspace, or a ledger table cannot be
    /// read.
    #[instrument(level = "debug", skip(self, pool, migrations), err(Debug))]
    pub async fn ledger(
        &self,
        pool: &CqlPool,
        migrations: &[(&str, &str)],
    ) -> Result<MigrationLedger, CqlError> {
        let keyspace = self.keyspace(pool)?;
        let present: Vec<(String,)> = pool
            .fetch_all(
                &crate::Cql::new(
                    "SELECT table_name FROM system_schema.tables WHERE keyspace_name = ?",
                )
                .bind(CqlValue::text(keyspace.as_str())),
            )
            .await?;
        let has = |table: &str| present.iter().any(|(name,)| name == table);
        let tables = StepTables::of(&keyspace)?;
        let applied: BTreeMap<String, DateTime<Utc>> = if has(TRACKING_TABLE) {
            pool.fetch_all::<(String, Option<DateTime<Utc>>)>(&cql!(
                "SELECT name, applied_at FROM {}",
                tracking(&keyspace)?
            ))
            .await?
            .into_iter()
            .map(|(name, at)| (name, at.unwrap_or_default()))
            .collect()
        } else {
            BTreeMap::new()
        };
        let steps = if has(STEPS_TABLE) {
            tables.steps(pool).await?
        } else {
            Vec::new()
        };
        let resolutions = if has(RESOLUTIONS_TABLE) {
            tables.resolutions(pool).await?
        } else {
            Vec::new()
        };
        Ok(MigrationLedger::read(
            migrations,
            &applied,
            &steps,
            &resolutions,
        ))
    }

    /// Applies all pending migrations from `migrations` (`(name, cql)` pairs) in
    /// order, skipping ones already applied. Returns the names applied by this call.
    ///
    /// # Errors
    /// Returns [`CqlError::MigrationFailed`] if one of a migration's statements fails, naming
    /// which step and carrying the driver's own error, recorded in the step ledger;
    /// [`LedgerError::Unresolved`] (as [`CqlError::Ledger`]) while a pending migration has a
    /// failed step nothing has settled, before anything runs;
    /// [`CqlError::MigrationRecordFailed`] if every statement applied and the ledger row did
    /// not; [`CqlError::MigrationLockHeld`] (`CQL_019`) if another migrator held the
    /// keyspace's lease past the wait budget, and [`CqlError::MigrationLockLost`]
    /// (`CQL_020`) if this one's lease expired or was taken before a statement, both
    /// retryable; and [`CqlError::Config`] if the session is not pinned to a keyspace.
    #[instrument(
        skip(self, pool, migrations),
        fields(candidates = migrations.len(), applied = Empty, skipped = Empty),
        err(Debug)
    )]
    pub async fn apply_all(
        &self,
        pool: &CqlPool,
        migrations: &[(&str, &str)],
    ) -> Result<Vec<String>, CqlError> {
        // Whole-set, before any statement runs: with no transaction, a refusal halfway would leave
        // statements applied and unrecorded.
        self.guard.inspect_unsharded(migrations)?;

        let keyspace = self.keyspace(pool)?;
        // Held until every migration is recorded, so a migrator that waited reads a finished
        // ledger.
        let lease = MigrationLease::acquire(pool, &keyspace).await?;
        let outcome = self.apply_leased(pool, &keyspace, migrations, &lease).await;
        lease.release(pool).await;
        outcome
    }

    /// [`apply_all`](Self::apply_all)'s work, under a held lease.
    #[expect(
        clippy::cognitive_complexity,
        reason = "one loop; each tracing event's expansion is counted"
    )]
    async fn apply_leased(
        &self,
        pool: &CqlPool,
        keyspace: &str,
        migrations: &[(&str, &str)],
        lease: &MigrationLease,
    ) -> Result<Vec<String>, CqlError> {
        let keyspace = keyspace.to_owned();
        let applied = self.applied_hashes(pool, &keyspace).await?;
        let tables = StepTables::of(&keyspace)?;
        tables.create(pool).await?;
        tables.settle_stopped(pool).await?;
        let steps = tables.steps(pool).await?;
        Self::refuse_unresolved(migrations, &applied, &steps)?;
        let mut newly_applied = Vec::new();
        let mut skipped = 0usize;

        for (name, cql) in migrations {
            if let Some(recorded) = applied.get(*name) {
                Self::unchanged(name, recorded, cql)?;
                debug!(migration = %name, "migration already applied; skipping");
                skipped += 1;
                continue;
            }
            info!(migration = %name, "applying ycql migration");
            let at = Step {
                keyspace: &keyspace,
                tables: &tables,
                lease,
                steps: &steps,
            };
            self.apply_one(pool, &at, (name, cql)).await?;
            newly_applied.push((*name).to_string());
        }

        let span = Span::current();
        span.record("applied", newly_applied.len());
        span.record("skipped", skipped);
        info!(
            applied = newly_applied.len(),
            skipped, "cql migrations up to date"
        );
        Ok(newly_applied)
    }

    /// Refuses `name` when the hash `recorded` for it is not `cql`'s.
    fn unchanged(name: &str, recorded: &str, cql: &str) -> Result<(), CqlError> {
        let found = content_hash(cql);
        if recorded == found {
            return Ok(());
        }
        Err(CqlError::MigrationChanged {
            name: name.to_owned(),
            applied: recorded.to_owned(),
            found,
        })
    }

    /// Refuses the whole set while a migration in it not `applied` has a failed step.
    fn refuse_unresolved(
        migrations: &[(&str, &str)],
        applied: &BTreeMap<String, String>,
        steps: &[StepRecord],
    ) -> Result<(), CqlError> {
        match migrations
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

    /// Applies each step of a migration that `steps` does not record done with the same
    /// statement, sequentially, then records it.
    #[instrument(skip_all, fields(migration = name, statements = Empty), err(Debug))]
    async fn apply_one(
        &self,
        pool: &CqlPool,
        at: &Step<'_>,
        (name, cql): (&str, &str),
    ) -> Result<(), CqlError> {
        let statements = split_statements(cql);
        let total = statements.len();
        Span::current().record("statements", total);

        for (index, stmt) in statements.iter().enumerate() {
            let number = index + 1;
            // CQL's driver rejects a trailing `;` on a single statement (unlike
            // SQL), so strip the terminator `split_statements` appends.
            let stmt = stmt.trim_end_matches(';').trim();
            if stmt.is_empty() {
                continue;
            }
            let hash = content_hash(stmt);
            if at.is_done(name, number, &hash) {
                debug!(
                    migration = name,
                    step = number,
                    "step already done; skipping"
                );
                continue;
            }
            // Before every statement, because a statement on this wire cannot be taken back:
            // one run by a migrator whose lease another has since taken is one run twice.
            at.lease.refresh(pool, name).await?;
            at.tables.start(pool, name, (number, total), &hash).await?;
            // `Ddl::interpolated`: a migration's text comes from a file on disk.
            if let Err(e) = pool.execute_ddl(&Ddl::interpolated(stmt.to_owned())).await {
                Self::record_failure(pool, at, (name, number), &e).await;
                return Err(CqlError::MigrationFailed {
                    name: name.to_string(),
                    step: number,
                    total,
                    statement: stmt.to_owned(),
                    cause: e.into(),
                });
            }
            at.tables.done(pool, name, number).await?;
        }

        self.record(pool, at.keyspace, name, cql).await?;
        info!("cql migration applied");
        Ok(())
    }

    /// Records step `number` of `name` failed with `error`. A write that cannot reach the node
    /// leaves the step `running`, which the next migrator to hold the lease records as stopped.
    #[expect(
        clippy::cognitive_complexity,
        reason = "straight-line; each tracing event's expansion is counted"
    )]
    async fn record_failure(
        pool: &CqlPool,
        at: &Step<'_>,
        (name, number): (&str, usize),
        error: &CqlError,
    ) {
        // No rollback in CQL: statements before this one stay applied.
        error!(
            migration = %name,
            step = number,
            "cql migration statement failed; earlier statements remain applied"
        );
        let message = error.to_string();
        let failure = StepFailure {
            code: Some(error.error_code()),
            error: &message,
            answer_lost: answer_lost(error),
        };
        if let Err(unrecorded) = at.tables.fail(pool, (name, number), &failure).await {
            error!(
                migration = %name,
                step = number,
                error = %unrecorded,
                "the failed step could not be recorded; the next migrator records it as stopped"
            );
        }
    }

    /// Records `name` applied with `cql`'s hash.
    async fn record(
        &self,
        pool: &CqlPool,
        keyspace: &str,
        name: &str,
        cql: &str,
    ) -> Result<(), CqlError> {
        let ledger = tracking(keyspace)?;
        // `IF NOT EXISTS` as the last line behind the lease: a record already there is one a
        // lost lease let another migrator write, and is reported rather than overwritten.
        let recorded = pool
            .execute_conditional(
                &cql!(
                    "INSERT INTO {} (name, applied_at, content_hash) \
                     VALUES (?, toTimestamp(now()), ?) IF NOT EXISTS",
                    ledger
                )
                .bind(CqlValue::Text(name.to_string()))
                .bind(CqlValue::Text(content_hash(cql))),
            )
            .await
            .map_err(|e| CqlError::MigrationRecordFailed {
                name: name.to_string(),
                cause: e.into(),
            })?;
        if !recorded {
            return Err(CqlError::MigrationLockLost {
                keyspace: keyspace.to_owned(),
                migration: name.to_string(),
            });
        }
        Ok(())
    }

    /// Settles the failed step of `migration` (`(name, cql)`) by running it and every step
    /// after it again, recording `resolution` first. Returns the step it ran from.
    ///
    /// # Errors
    /// [`LedgerError::NothingToResolve`] when the migration is applied or has no failed step;
    /// [`LedgerError::NotRerunnable`] when the failed step's answer was lost and its statement
    /// cannot run again; [`CqlError::MigrationFailed`] when a step fails again, recorded as
    /// before; and the lease's errors, as [`apply_all`](Self::apply_all).
    #[instrument(skip(self, pool, cql, resolution), fields(by = resolution.by()), err(Debug))]
    pub async fn retry(
        &self,
        pool: &CqlPool,
        (name, cql): (&str, &str),
        resolution: &Resolution,
    ) -> Result<StepRecord, CqlError> {
        let keyspace = self.keyspace(pool)?;
        let lease = MigrationLease::acquire(pool, &keyspace).await?;
        let outcome = async {
            let tables = StepTables::of(&keyspace)?;
            let (failed, steps) = self.unsettled(pool, &keyspace, &tables, name).await?;
            if failed.answer_lost {
                let statement = split_statements(cql)
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
            tables
                .record(pool, &failed, Resolve::Retry, resolution)
                .await?;
            info!(
                migration = name,
                step = failed.step,
                "retrying a failed migration"
            );
            let at = Step {
                keyspace: &keyspace,
                tables: &tables,
                lease: &lease,
                steps: &steps,
            };
            self.apply_one(pool, &at, (name, cql)).await?;
            Ok(failed)
        }
        .await;
        lease.release(pool).await;
        outcome
    }

    /// Settles the failed step of `migration` (`(name, cql)`) by recording it applied, which
    /// `proof` shows the live catalog already holds, with `resolution`. Returns the step it
    /// settled.
    ///
    /// # Errors
    /// [`LedgerError::NothingToResolve`] when the migration is applied or has no failed step;
    /// and the lease's errors, as [`apply_all`](Self::apply_all).
    #[instrument(skip(self, pool, cql, resolution, proof), fields(by = resolution.by(), effects = proof.effects()), err(Debug))]
    pub async fn mark_applied(
        &self,
        pool: &CqlPool,
        (name, cql): (&str, &str),
        resolution: &Resolution,
        proof: &CatalogProof,
    ) -> Result<StepRecord, CqlError> {
        let keyspace = self.keyspace(pool)?;
        let lease = MigrationLease::acquire(pool, &keyspace).await?;
        let outcome = async {
            let tables = StepTables::of(&keyspace)?;
            let (failed, _) = self.unsettled(pool, &keyspace, &tables, name).await?;
            tables
                .record(pool, &failed, Resolve::Applied, resolution)
                .await?;
            lease.refresh(pool, name).await?;
            self.record(pool, &keyspace, name, cql).await?;
            info!(
                migration = name,
                "a failed migration recorded as applied, its catalog proved"
            );
            Ok(failed)
        }
        .await;
        lease.release(pool).await;
        outcome
    }

    /// `name`'s failed step and every step row, or [`LedgerError::NothingToResolve`].
    async fn unsettled(
        &self,
        pool: &CqlPool,
        keyspace: &str,
        tables: &StepTables,
        name: &str,
    ) -> Result<(StepRecord, Vec<StepRecord>), CqlError> {
        tables.create(pool).await?;
        tables.settle_stopped(pool).await?;
        let nothing = || LedgerError::NothingToResolve {
            name: name.to_owned(),
        };
        if self
            .applied_hashes(pool, keyspace)
            .await?
            .contains_key(name)
        {
            return Err(nothing().into());
        }
        let steps = tables.steps(pool).await?;
        let failed = failed_step(&steps, name).cloned().ok_or_else(nothing)?;
        Ok((failed, steps))
    }

    /// Every applied migration with the content hash recorded for it.
    async fn applied_hashes(
        &self,
        pool: &CqlPool,
        keyspace: &str,
    ) -> Result<BTreeMap<String, String>, CqlError> {
        self.ensure_tracking_table(pool, keyspace).await?;
        let ledger = tracking(keyspace)?;
        Ok(pool
            .fetch_all::<(String, String)>(&cql!("SELECT name, content_hash FROM {}", ledger))
            .await?
            .into_iter()
            .collect())
    }

    /// Resolves the keyspace the session is pinned to.
    fn keyspace(&self, pool: &CqlPool) -> Result<String, CqlError> {
        pool.session()
            .inner()
            .get_keyspace()
            .map(|ks| ks.to_string())
            // A configuration mistake, not a migration failure: nothing was attempted.
            .ok_or_else(|| {
                CqlError::config("the session has no keyspace selected; migrations need a keyspace")
            })
    }

    #[instrument(level = "trace", skip(self, pool), err(Debug))]
    async fn ensure_tracking_table(&self, pool: &CqlPool, keyspace: &str) -> Result<(), CqlError> {
        let ledger = tracking(keyspace)?;
        pool.execute_ddl(&ddl!(
            "CREATE TABLE IF NOT EXISTS {} \
             (name text PRIMARY KEY, applied_at timestamp, content_hash text)",
            ledger
        ))
        .await
    }
}

/// Where the steps of one migration run: the keyspace, its step tables and the lease held on it.
struct Step<'a> {
    keyspace: &'a str,
    tables: &'a StepTables,
    lease: &'a MigrationLease,
    /// Every step row, as read when the lease was taken.
    steps: &'a [StepRecord],
}

impl Step<'_> {
    /// Whether step `number` of `name` is recorded done with the statement whose hash is `hash`.
    fn is_done(&self, name: &str, number: usize, hash: &str) -> bool {
        self.steps.iter().any(|record| {
            record.name == name
                && record.step == number
                && record.state == StepState::Done
                && record.statement_hash == hash
        })
    }
}

/// `name`'s failed step in `steps`.
fn failed_step<'s>(steps: &'s [StepRecord], name: &str) -> Option<&'s StepRecord> {
    steps
        .iter()
        .find(|record| record.name == name && record.state == StepState::Failed)
}

/// Whether `error` leaves it unknown whether the statement applied: a timeout or a broken
/// connection reports nothing about the statement, where a refusal — an unavailable replica, a
/// rate limit, a statement the node rejected — says it did not run.
fn answer_lost(error: &CqlError) -> bool {
    error.is_retryable()
        && !matches!(
            error,
            CqlError::Unavailable { .. } | CqlError::RateLimited(_)
        )
}
