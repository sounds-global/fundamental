//! [`MigrationLease`] — one migrator at a time per keyspace, on a wire with no transactions.

use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use crate::pool::CqlPool;
use crate::retry::calculate_delay;
use crate::{CqlError, CqlIdent, CqlValue, RetryConfig, cql, ddl};

/// The lock table, beside the ledger in the keyspace it guards.
const LOCK_TABLE: &str = "_fundamental_migration_lock";

/// The one row every migrator of a keyspace contends for.
const LOCK_ROW: &str = "ledger";

/// How long a lease lives unrefreshed: long enough for one migration statement, short enough
/// that a migrator which died holding it stops blocking the next deploy within minutes.
const LEASE_TTL_SECS: i32 = 120;

/// How long a migrator waits for another to finish before giving up.
const WAIT_BUDGET: Duration = Duration::from_secs(600);

/// The first wait between attempts, doubled up to [`WAIT_CEILING`] and jittered, so migrators
/// that started together do not ask again in step.
const WAIT_FLOOR: Duration = Duration::from_millis(100);
const WAIT_CEILING: Duration = Duration::from_secs(2);

/// A held lease on a keyspace's migrations.
///
/// YCQL has no transaction to lock in, so the lock is a TTL'd row holding a random token,
/// refreshed before every statement with `IF holder = <token>`.
pub(crate) struct MigrationLease {
    table: CqlIdent,
    keyspace: String,
    holder: String,
}

impl MigrationLease {
    /// Waits for the keyspace's lease and takes it.
    ///
    /// # Errors
    /// [`CqlError::MigrationLockHeld`] when another migrator holds it past the wait budget;
    /// the driver's error when the lock table cannot be created or written.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) async fn acquire(pool: &CqlPool, keyspace: &str) -> Result<Self, CqlError> {
        let table = CqlIdent::qualified(keyspace, LOCK_TABLE)?;
        create_lock_table(pool, &table).await?;
        let holder = uuid::Uuid::new_v4().simple().to_string();
        let started = Instant::now();
        let schedule = RetryConfig::new()
            .base_delay(WAIT_FLOOR)
            .max_delay(WAIT_CEILING);
        let mut attempt = 0u32;
        loop {
            let taken = pool
                .execute_conditional(
                    &cql!(
                        "INSERT INTO {} (name, holder) VALUES (?, ?) IF NOT EXISTS USING TTL ?",
                        table
                    )
                    .bind(CqlValue::Text(LOCK_ROW.to_owned()))
                    .bind(CqlValue::Text(holder.clone()))
                    .bind(CqlValue::Int(LEASE_TTL_SECS)),
                )
                .await?;
            if taken {
                info!(keyspace, "took the migration lock");
                return Ok(Self {
                    table,
                    keyspace: keyspace.to_owned(),
                    holder,
                });
            }
            let waited = started.elapsed();
            if waited >= WAIT_BUDGET {
                return Err(CqlError::MigrationLockHeld {
                    keyspace: keyspace.to_owned(),
                    waited_secs: waited.as_secs(),
                });
            }
            attempt += 1;
            if attempt == 1 {
                info!(
                    keyspace,
                    "another migrator holds the lock; waiting for the migration lock"
                );
            } else {
                debug!(
                    keyspace,
                    waited_ms = waited.as_millis(),
                    "still waiting for the migration lock"
                );
            }
            tokio::time::sleep(calculate_delay(&schedule, attempt)).await;
        }
    }

    /// Extends the lease before `migration`'s next statement.
    ///
    /// # Errors
    /// [`CqlError::MigrationLockLost`] when the lease expired or another migrator holds it.
    pub(crate) async fn refresh(&self, pool: &CqlPool, migration: &str) -> Result<(), CqlError> {
        let held = pool
            .execute_conditional(
                &cql!(
                    "UPDATE {} USING TTL ? SET holder = ? WHERE name = ? IF holder = ?",
                    self.table
                )
                .bind(CqlValue::Int(LEASE_TTL_SECS))
                .bind(CqlValue::Text(self.holder.clone()))
                .bind(CqlValue::Text(LOCK_ROW.to_owned()))
                .bind(CqlValue::Text(self.holder.clone())),
            )
            .await?;
        if held {
            return Ok(());
        }
        Err(CqlError::MigrationLockLost {
            keyspace: self.keyspace.clone(),
            migration: migration.to_owned(),
        })
    }

    /// Gives the lease back. A release that fails is logged: the lease then ends at its TTL.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) async fn release(self, pool: &CqlPool) {
        let released = pool
            .execute_conditional(
                &cql!("DELETE FROM {} WHERE name = ? IF holder = ?", self.table)
                    .bind(CqlValue::Text(LOCK_ROW.to_owned()))
                    .bind(CqlValue::Text(self.holder.clone())),
            )
            .await;
        match released {
            Ok(true) => info!(keyspace = %self.keyspace, "released the migration lock"),
            Ok(false) => warn!(
                keyspace = %self.keyspace,
                "the migration lock was no longer this migrator's to release"
            ),
            Err(failure) => warn!(
                keyspace = %self.keyspace,
                code = fundamental_core::Error::error_code(&failure),
                error = %failure,
                "could not release the migration lock; it ends at its TTL"
            ),
        }
    }
}

/// Creates the lock table, once more if the first attempt lost a race with another migrator
/// creating it — the second `IF NOT EXISTS` then finds it.
async fn create_lock_table(pool: &CqlPool, table: &CqlIdent) -> Result<(), CqlError> {
    let create = ddl!(
        "CREATE TABLE IF NOT EXISTS {} (name text PRIMARY KEY, holder text)",
        table
    );
    match pool.execute_ddl(&create).await {
        Ok(()) => Ok(()),
        Err(first) => {
            debug!(
                code = fundamental_core::Error::error_code(&first),
                error = %first,
                "creating the migration lock table lost a race; asking again"
            );
            pool.execute_ddl(&create).await
        }
    }
}
