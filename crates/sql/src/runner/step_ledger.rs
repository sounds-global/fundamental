use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use fundamental_db::migration::{
    MIGRATIONS_TABLE, RESOLUTIONS_TABLE, Resolution, Resolve, Resolved, STEPS_TABLE, StepRecord,
    StepState,
};
use sqlx::pool::PoolConnection;
use sqlx::postgres::PgRow;
use sqlx::{Connection as _, Postgres, Row as _};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use super::MIGRATION_ATTEMPTS;
use crate::SqlError;
use crate::pool::SqlPool;

/// The ledger's three tables, qualified with the pool's validated schema.
pub(super) struct Tables {
    pub(super) migrations: String,
    pub(super) steps: String,
    pub(super) resolutions: String,
}

impl Tables {
    pub(super) fn of(pool: &SqlPool) -> Self {
        let schema = pool.schema();
        Self {
            migrations: schema.qualify(MIGRATIONS_TABLE),
            steps: schema.qualify(STEPS_TABLE),
            resolutions: schema.qualify(RESOLUTIONS_TABLE),
        }
    }

    pub(super) fn select_applied(&self) -> String {
        format!(
            "SELECT name, applied_at, content_hash FROM {}",
            self.migrations
        )
    }

    pub(super) fn select_steps(&self) -> String {
        format!(
            "SELECT name, step, total, statement_hash, state, code, error, answer_lost, at \
             FROM {} ORDER BY name, step",
            self.steps
        )
    }

    pub(super) fn select_resolutions(&self) -> String {
        format!(
            "SELECT name, act, resolved_by, reason, login, step, code, at FROM {} ORDER BY at",
            self.resolutions
        )
    }
}

pub(super) fn applied_rows(
    rows: &[PgRow],
) -> Result<BTreeMap<String, (DateTime<Utc>, String)>, SqlError> {
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get::<String, _>("name")?,
                (
                    row.try_get::<DateTime<Utc>, _>("applied_at")?,
                    row.try_get::<String, _>("content_hash")?,
                ),
            ))
        })
        .collect()
}

pub(super) fn step_rows(rows: &[PgRow]) -> Result<Vec<StepRecord>, SqlError> {
    rows.iter()
        .map(|row| {
            let state: String = row.try_get("state")?;
            Ok(StepRecord {
                name: row.try_get("name")?,
                step: count(row.try_get::<i32, _>("step")?),
                total: count(row.try_get::<i32, _>("total")?),
                statement_hash: row.try_get("statement_hash")?,
                state: StepState::parse(&state).ok_or_else(|| {
                    SqlError::Introspection(format!("a step row in the state {state:?}"))
                })?,
                code: row.try_get("code")?,
                error: row.try_get("error")?,
                answer_lost: row.try_get("answer_lost")?,
                at: row.try_get("at")?,
            })
        })
        .collect()
}

pub(super) fn resolution_rows(rows: &[PgRow]) -> Result<Vec<Resolved>, SqlError> {
    rows.iter()
        .map(|row| {
            let act: String = row.try_get("act")?;
            Ok(Resolved {
                name: row.try_get("name")?,
                act: Resolve::parse(&act).ok_or_else(|| {
                    SqlError::Introspection(format!("a resolution recorded as {act:?}"))
                })?,
                by: row.try_get("resolved_by")?,
                reason: row.try_get("reason")?,
                login: row.try_get("login")?,
                step: count(row.try_get::<i32, _>("step")?),
                code: row.try_get("code")?,
                at: row.try_get("at")?,
            })
        })
        .collect()
}

fn count(stored: i32) -> usize {
    usize::try_from(stored).unwrap_or(0)
}

fn stored(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

/// The server's message for a failure when it gave one, else what this process knows.
pub(super) fn engine_message(error: &SqlError) -> String {
    match error {
        SqlError::Sqlx(sqlx::Error::Database(db)) => db.message().to_owned(),
        SqlError::Sqlx(other) => other.to_string(),
        other => other.to_string(),
    }
}

/// What one step of a migration is: where it is in the migration, and the statement.
pub(super) struct Step<'a> {
    pub(super) name: &'a str,
    pub(super) number: usize,
    pub(super) total: usize,
    pub(super) statement: &'a str,
    pub(super) hash: String,
}

/// One pooled connection holding the session's migration lock for a whole apply or resolution.
///
/// Each step commits on its own, so a transaction-scoped lock would let another replica in
/// between steps. The connection is closed, never returned, so the lock cannot ride into another session.
pub(super) struct StepLedger<'p> {
    pool: &'p SqlPool,
    conn: Mutex<PoolConnection<Postgres>>,
    lock: i64,
    pub(super) tables: Tables,
}

impl<'p> StepLedger<'p> {
    /// Takes a connection, waits for the migration lock on it, and creates the ledger's tables if absent.
    pub(super) async fn open(pool: &'p SqlPool, lock: i64) -> Result<Self, SqlError> {
        let mut conn = pool.connector().acquire().await?;
        conn.close_on_drop();
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(lock)
            .execute(&mut *conn)
            .await?;
        debug!("took the migration lock");
        let ledger = Self {
            pool,
            conn: Mutex::new(conn),
            lock,
            tables: Tables::of(pool),
        };
        ledger.create_tables().await?;
        Ok(ledger)
    }

    /// Releases the lock and closes the connection; the close releases a lock that could not be.
    #[expect(
        clippy::cognitive_complexity,
        reason = "straight-line; each tracing event's expansion is counted"
    )]
    pub(super) async fn close(self) {
        let mut conn = self.conn.into_inner();
        if let Err(error) = sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(self.lock)
            .execute(&mut *conn)
            .await
        {
            warn!(%error, "the migration lock was not released; closing its connection releases it");
        }
        if let Err(error) = conn.close().await {
            debug!(%error, "the migration connection did not close cleanly");
        }
    }

    async fn execute(&self, sql: String) -> Result<(), SqlError> {
        self.pool
            .with_retry_budget(MIGRATION_ATTEMPTS, || async {
                let mut conn = self.conn.lock().await;
                sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                    .execute(&mut **conn)
                    .await?;
                Ok(())
            })
            .await
    }

    pub(super) async fn fetch(&self, sql: String) -> Result<Vec<PgRow>, SqlError> {
        self.pool
            .with_retry_budget(MIGRATION_ATTEMPTS, || async {
                let mut conn = self.conn.lock().await;
                Ok(sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                    .fetch_all(&mut **conn)
                    .await?)
            })
            .await
    }

    /// Creates the migrations and steps tables on this connection; the resolutions table waits for the first resolution.
    async fn create_tables(&self) -> Result<(), SqlError> {
        let Tables {
            migrations, steps, ..
        } = &self.tables;
        self.execute(format!(
            "CREATE TABLE IF NOT EXISTS {migrations} (\
             name TEXT NOT NULL PRIMARY KEY, \
             applied_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
             content_hash TEXT NOT NULL)"
        ))
        .await?;
        self.execute(format!(
            "CREATE TABLE IF NOT EXISTS {steps} (\
             name TEXT NOT NULL, step INT NOT NULL, total INT NOT NULL, \
             statement_hash TEXT NOT NULL, state TEXT NOT NULL, code TEXT, error TEXT, \
             answer_lost BOOLEAN NOT NULL DEFAULT false, \
             at TIMESTAMPTZ NOT NULL DEFAULT now(), \
             PRIMARY KEY (name HASH, step ASC))"
        ))
        .await
    }

    async fn create_resolutions(&self) -> Result<(), SqlError> {
        self.execute(format!(
            "CREATE TABLE IF NOT EXISTS {} (\
             name TEXT NOT NULL, at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), \
             act TEXT NOT NULL, resolved_by TEXT NOT NULL, reason TEXT NOT NULL, \
             login TEXT NOT NULL DEFAULT current_user, step INT NOT NULL, code TEXT, \
             PRIMARY KEY (name HASH, at ASC))",
            self.tables.resolutions
        ))
        .await
    }

    pub(super) async fn applied(
        &self,
    ) -> Result<BTreeMap<String, (DateTime<Utc>, String)>, SqlError> {
        applied_rows(&self.fetch(self.tables.select_applied()).await?)
    }

    pub(super) async fn steps(&self) -> Result<Vec<StepRecord>, SqlError> {
        step_rows(&self.fetch(self.tables.select_steps()).await?)
    }

    /// Records every step still `running` as failed with its answer lost: the migrator that started it has stopped.
    pub(super) async fn settle_stopped(&self) -> Result<(), SqlError> {
        self.execute(format!(
            "UPDATE {} SET state = 'failed', answer_lost = true, at = now(), \
             error = 'the migrator stopped before the step''s answer arrived' \
             WHERE state = 'running'",
            self.tables.steps
        ))
        .await
    }

    /// Runs again, unrecorded, a setting a done step made, as a resumed migration needs it.
    pub(super) async fn reapply(&self, setting: &str) -> Result<(), SqlError> {
        self.execute(setting.to_owned()).await
    }

    /// Records `step` as started, before its statement is sent.
    pub(super) async fn start(&self, step: &Step<'_>) -> Result<(), SqlError> {
        let sql = format!(
            "INSERT INTO {} (name, step, total, statement_hash, state, code, error, \
             answer_lost, at) VALUES ($1, $2, $3, $4, 'running', NULL, NULL, false, now()) \
             ON CONFLICT (name, step) DO UPDATE SET total = EXCLUDED.total, \
             statement_hash = EXCLUDED.statement_hash, state = 'running', code = NULL, \
             error = NULL, answer_lost = false, at = now()",
            self.tables.steps
        );
        self.pool
            .with_retry_budget(MIGRATION_ATTEMPTS, || async {
                let mut conn = self.conn.lock().await;
                sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                    .bind(step.name)
                    .bind(stored(step.number))
                    .bind(stored(step.total))
                    .bind(&step.hash)
                    .execute(&mut **conn)
                    .await?;
                Ok(())
            })
            .await
    }

    /// Runs `step`'s statement and records it done in one transaction, or alone first when it cannot run in one.
    pub(super) async fn run(&self, step: &Step<'_>, content_hash: &str) -> Result<(), SqlError> {
        let alone = runs_alone(step.statement);
        let done = format!(
            "UPDATE {} SET state = 'done', at = now() WHERE name = $1 AND step = $2",
            self.tables.steps
        );
        let record = format!(
            "INSERT INTO {} (name, applied_at, content_hash) VALUES ($1, now(), $2) \
             ON CONFLICT (name) DO NOTHING",
            self.tables.migrations
        );
        self.pool
            .with_retry_budget(MIGRATION_ATTEMPTS, || async {
                let mut conn = self.conn.lock().await;
                if alone {
                    sqlx::query(sqlx::AssertSqlSafe(step.statement))
                        .execute(&mut **conn)
                        .await?;
                }
                let mut tx = conn.begin().await?;
                if !alone {
                    sqlx::query(sqlx::AssertSqlSafe(step.statement))
                        .execute(&mut *tx)
                        .await?;
                }
                sqlx::query(sqlx::AssertSqlSafe(done.as_str()))
                    .bind(step.name)
                    .bind(stored(step.number))
                    .execute(&mut *tx)
                    .await?;
                if step.number == step.total {
                    sqlx::query(sqlx::AssertSqlSafe(record.as_str()))
                        .bind(step.name)
                        .bind(content_hash)
                        .execute(&mut *tx)
                        .await?;
                }
                tx.commit().await?;
                Ok(())
            })
            .await
    }

    /// Records a migration with no statements as applied.
    pub(super) async fn record_empty(
        &self,
        name: &str,
        content_hash: &str,
    ) -> Result<(), SqlError> {
        let sql = format!(
            "INSERT INTO {} (name, applied_at, content_hash) VALUES ($1, now(), $2) \
             ON CONFLICT (name) DO NOTHING",
            self.tables.migrations
        );
        self.pool
            .with_retry_budget(MIGRATION_ATTEMPTS, || async {
                let mut conn = self.conn.lock().await;
                sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                    .bind(name)
                    .bind(content_hash)
                    .execute(&mut **conn)
                    .await?;
                Ok(())
            })
            .await
    }

    /// Records `step` as failed after its transaction rolled back; if that write fails, the next migrator records it.
    pub(super) async fn fail(&self, step: &Step<'_>, error: &SqlError, answer_lost: bool) {
        let sql = format!(
            "UPDATE {} SET state = 'failed', code = $3, error = $4, answer_lost = $5, \
             at = now() WHERE name = $1 AND step = $2",
            self.tables.steps
        );
        let code = error.aborting_sqlstate();
        let message = engine_message(error);
        let written = self
            .pool
            .with_retry_budget(MIGRATION_ATTEMPTS, || async {
                let mut conn = self.conn.lock().await;
                sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                    .bind(step.name)
                    .bind(stored(step.number))
                    .bind(code.as_deref())
                    .bind(&message)
                    .bind(answer_lost)
                    .execute(&mut **conn)
                    .await?;
                Ok(())
            })
            .await;
        if let Err(unwritten) = written {
            warn!(
                migration = step.name,
                step = step.number,
                error = %unwritten,
                "the failed step could not be recorded; the next migrator records it as stopped"
            );
        }
    }

    /// Forgets `step`'s start: it rolled back on a conflict, so running it again is a first run.
    pub(super) async fn forget(&self, step: &Step<'_>) {
        let sql = format!(
            "DELETE FROM {} WHERE name = $1 AND step = $2 AND state = 'running'",
            self.tables.steps
        );
        let forgotten = self
            .pool
            .with_retry_budget(MIGRATION_ATTEMPTS, || async {
                let mut conn = self.conn.lock().await;
                sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                    .bind(step.name)
                    .bind(stored(step.number))
                    .execute(&mut **conn)
                    .await?;
                Ok(())
            })
            .await;
        if let Err(error) = forgotten {
            warn!(migration = step.name, step = step.number, %error, "a step's start was not forgotten");
        }
    }

    /// Records `resolution` of the failure at `failed`, in one transaction with the act's own ledger change.
    pub(super) async fn resolve(
        &self,
        failed: &StepRecord,
        act: Resolve,
        resolution: &Resolution,
        content_hash: &str,
    ) -> Result<(), SqlError> {
        self.create_resolutions().await?;
        let record = format!(
            "INSERT INTO {} (name, at, act, resolved_by, reason, login, step, code) \
             VALUES ($1, clock_timestamp(), $2, $3, $4, current_user, $5, $6)",
            self.tables.resolutions
        );
        let change = match act {
            Resolve::Retry => format!(
                "UPDATE {} SET state = 'running', at = now() WHERE name = $1 AND step = $2",
                self.tables.steps
            ),
            Resolve::Applied => format!(
                "INSERT INTO {} (name, applied_at, content_hash) VALUES ($1, now(), $2) \
                 ON CONFLICT (name) DO NOTHING",
                self.tables.migrations
            ),
        };
        self.pool
            .with_retry_budget(MIGRATION_ATTEMPTS, || async {
                let mut conn = self.conn.lock().await;
                let mut tx = conn.begin().await?;
                sqlx::query(sqlx::AssertSqlSafe(record.as_str()))
                    .bind(&failed.name)
                    .bind(act.as_str())
                    .bind(resolution.by())
                    .bind(resolution.reason())
                    .bind(stored(failed.step))
                    .bind(failed.code.as_deref())
                    .execute(&mut *tx)
                    .await?;
                let query = sqlx::query(sqlx::AssertSqlSafe(change.as_str())).bind(&failed.name);
                let query = match act {
                    Resolve::Retry => query.bind(stored(failed.step)),
                    Resolve::Applied => query.bind(content_hash),
                };
                query.execute(&mut *tx).await?;
                tx.commit().await?;
                Ok(())
            })
            .await
    }
}

/// Whether `statement` cannot run inside a transaction block: a `CONCURRENTLY` index build or drop.
fn runs_alone(statement: &str) -> bool {
    statement
        .split_whitespace()
        .take(4)
        .any(|word| word.eq_ignore_ascii_case("CONCURRENTLY"))
}

#[cfg(test)]
mod tests {
    use super::runs_alone;

    #[test]
    fn a_concurrent_index_build_runs_alone_and_nothing_else_does() {
        assert!(runs_alone(
            "CREATE INDEX CONCURRENTLY IF NOT EXISTS by_customer ON orders (customer_id);"
        ));
        assert!(runs_alone("DROP INDEX CONCURRENTLY IF EXISTS by_customer;"));
        assert!(!runs_alone(
            "CREATE INDEX IF NOT EXISTS concurrently_named ON orders (customer_id);"
        ));
    }
}
