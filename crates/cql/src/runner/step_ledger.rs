//! The YCQL step ledger: every read and write of `_fundamental_migration_steps` and
//! `_fundamental_migration_resolutions`, beside the migrations table in one keyspace.
//!
//! There is no transaction on this wire, so each write stands alone and the migration lease is
//! what keeps a second migrator out between them.

use chrono::{DateTime, Utc};
use fundamental_db::migration::{
    RESOLUTIONS_TABLE, Resolution, Resolve, Resolved, STEPS_TABLE, StepRecord, StepState,
};

use crate::pool::CqlPool;
use crate::{CqlError, CqlIdent, CqlValue, cql, ddl};

/// A step row as the driver reads it.
type StepRow = (
    String,
    i32,
    i32,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<bool>,
    Option<DateTime<Utc>>,
);

/// A resolution row as the driver reads it.
type ResolutionRow = (
    String,
    DateTime<Utc>,
    String,
    String,
    String,
    Option<String>,
    i32,
    Option<String>,
);

/// How a step failed: the error's code, its message, and whether the answer was lost.
#[derive(Clone, Copy)]
pub(super) struct StepFailure<'a> {
    pub(super) code: Option<&'a str>,
    pub(super) error: &'a str,
    pub(super) answer_lost: bool,
}

/// The keyspace's two step-ledger tables, each quoted as CQL requires of a leading `_`.
pub(super) struct StepTables {
    steps: CqlIdent,
    resolutions: CqlIdent,
}

impl StepTables {
    /// The tables in `keyspace`.
    ///
    /// # Errors
    /// [`CqlError::InvalidIdentifier`] when `keyspace` is not a plain identifier.
    pub(super) fn of(keyspace: &str) -> Result<Self, CqlError> {
        Ok(Self {
            steps: CqlIdent::qualified(keyspace, STEPS_TABLE)?,
            resolutions: CqlIdent::qualified(keyspace, RESOLUTIONS_TABLE)?,
        })
    }

    /// Creates the steps table if it is not there. The resolutions table is made by the first
    /// resolution, so a fresh keyspace pays for one catalog write and not two.
    pub(super) async fn create(&self, pool: &CqlPool) -> Result<(), CqlError> {
        pool.execute_ddl(&ddl!(
            "CREATE TABLE IF NOT EXISTS {} (name text, step int, total int, \
             statement_hash text, state text, code text, error text, answer_lost boolean, \
             at timestamp, PRIMARY KEY ((name), step))",
            self.steps
        ))
        .await
    }

    /// Creates the resolutions table if it is not there.
    pub(super) async fn create_resolutions(&self, pool: &CqlPool) -> Result<(), CqlError> {
        pool.execute_ddl(&ddl!(
            "CREATE TABLE IF NOT EXISTS {} (name text, at timestamp, act text, \
             resolved_by text, reason text, login text, step int, code text, \
             PRIMARY KEY ((name), at))",
            self.resolutions
        ))
        .await
    }

    /// Every step row.
    pub(super) async fn steps(&self, pool: &CqlPool) -> Result<Vec<StepRecord>, CqlError> {
        pool.fetch_all::<StepRow>(&cql!(
            "SELECT name, step, total, statement_hash, state, code, error, answer_lost, at \
             FROM {}",
            self.steps
        ))
        .await?
        .into_iter()
        .map(
            |(name, step, total, statement_hash, state, code, error, answer_lost, at)| {
                Ok(StepRecord {
                    name,
                    step: count(step),
                    total: count(total),
                    statement_hash,
                    state: StepState::parse(&state).ok_or_else(|| {
                        CqlError::Introspection(format!("a step row in the state {state:?}"))
                    })?,
                    code,
                    error,
                    answer_lost: answer_lost.unwrap_or(false),
                    at: at.unwrap_or_default(),
                })
            },
        )
        .collect()
    }

    /// Every resolution row.
    pub(super) async fn resolutions(&self, pool: &CqlPool) -> Result<Vec<Resolved>, CqlError> {
        pool.fetch_all::<ResolutionRow>(&cql!(
            "SELECT name, at, act, resolved_by, reason, login, step, code FROM {}",
            self.resolutions
        ))
        .await?
        .into_iter()
        .map(|(name, at, act, by, reason, login, step, code)| {
            Ok(Resolved {
                name,
                act: Resolve::parse(&act).ok_or_else(|| {
                    CqlError::Introspection(format!("a resolution recorded as {act:?}"))
                })?,
                by,
                reason,
                login,
                step: count(step),
                code,
                at,
            })
        })
        .collect()
    }

    /// Records every step still `running` as failed with its answer lost: this migrator holds
    /// the lease, so the one that started it has stopped.
    pub(super) async fn settle_stopped(&self, pool: &CqlPool) -> Result<(), CqlError> {
        for stopped in self
            .steps(pool)
            .await?
            .into_iter()
            .filter(|record| record.state == StepState::Running)
        {
            self.fail(
                pool,
                (&stopped.name, stopped.step),
                &StepFailure {
                    code: None,
                    error: "the migrator stopped before the step's answer arrived",
                    answer_lost: true,
                },
            )
            .await?;
        }
        Ok(())
    }

    /// Records step `step` of `total` of `name` as started, before its statement is sent.
    pub(super) async fn start(
        &self,
        pool: &CqlPool,
        name: &str,
        (step, total): (usize, usize),
        statement_hash: &str,
    ) -> Result<(), CqlError> {
        pool.execute(
            &cql!(
                "INSERT INTO {} (name, step, total, statement_hash, state, code, error, \
                 answer_lost, at) VALUES (?, ?, ?, ?, 'running', null, null, false, \
                 toTimestamp(now()))",
                self.steps
            )
            .bind(CqlValue::Text(name.to_owned()))
            .bind(CqlValue::Int(stored(step)))
            .bind(CqlValue::Int(stored(total)))
            .bind(CqlValue::Text(statement_hash.to_owned())),
        )
        .await
    }

    /// Records step `step` of `name` as done.
    pub(super) async fn done(
        &self,
        pool: &CqlPool,
        name: &str,
        step: usize,
    ) -> Result<(), CqlError> {
        pool.execute(
            &cql!(
                "UPDATE {} SET state = 'done', at = toTimestamp(now()) WHERE name = ? AND step = ?",
                self.steps
            )
            .bind(CqlValue::Text(name.to_owned()))
            .bind(CqlValue::Int(stored(step))),
        )
        .await
    }

    /// Records step `step` of `name` as failed, as `failure` says.
    pub(super) async fn fail(
        &self,
        pool: &CqlPool,
        (name, step): (&str, usize),
        failure: &StepFailure<'_>,
    ) -> Result<(), CqlError> {
        let StepFailure {
            code,
            error,
            answer_lost,
        } = *failure;
        pool.execute(
            &cql!(
                "UPDATE {} SET state = 'failed', code = ?, error = ?, answer_lost = ?, \
                 at = toTimestamp(now()) WHERE name = ? AND step = ?",
                self.steps
            )
            .bind(code.map_or(CqlValue::Null, |code| CqlValue::Text(code.to_owned())))
            .bind(CqlValue::Text(error.to_owned()))
            .bind(CqlValue::Bool(answer_lost))
            .bind(CqlValue::Text(name.to_owned()))
            .bind(CqlValue::Int(stored(step))),
        )
        .await
    }

    /// Records `resolution` of the failure at `failed`, as `act`. The act's own ledger change
    /// follows under the same lease: there is no transaction to put both in.
    pub(super) async fn record(
        &self,
        pool: &CqlPool,
        failed: &StepRecord,
        act: Resolve,
        resolution: &Resolution,
    ) -> Result<(), CqlError> {
        self.create_resolutions(pool).await?;
        pool.execute(
            &cql!(
                "INSERT INTO {} (name, at, act, resolved_by, reason, login, step, code) \
                 VALUES (?, toTimestamp(now()), ?, ?, ?, null, ?, ?)",
                self.resolutions
            )
            .bind(CqlValue::Text(failed.name.clone()))
            .bind(CqlValue::Text(act.as_str().to_owned()))
            .bind(CqlValue::Text(resolution.by().to_owned()))
            .bind(CqlValue::Text(resolution.reason().to_owned()))
            .bind(CqlValue::Int(stored(failed.step)))
            .bind(failed.code.clone().map_or(CqlValue::Null, CqlValue::Text)),
        )
        .await
    }
}

/// A stored step number as a count; the ledger never writes a negative one.
fn count(stored: i32) -> usize {
    usize::try_from(stored).unwrap_or(0)
}

/// A count as the `int` column stores it.
fn stored(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}
