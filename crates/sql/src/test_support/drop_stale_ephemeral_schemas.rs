use std::collections::HashSet;

use fundamental_db::ephemeral::{EphemeralName, HeldName};
use sqlx::postgres::PgConnection;
use tokio::time::timeout;
use tracing::{instrument, warn};

use super::connect_dedicated::{close_dedicated, connect_dedicated};
use super::drop_schema_on::drop_schema_on;
use super::try_advisory_lock::try_advisory_lock;
use super::{SweepBudget, SweepOutcome};
use crate::{SqlError, SqlResult};

/// The advisory key a sweep holds on a database, so one sweeper runs per database; it can never equal a held name's key.
const SWEEPER_KEY: (i32, i32) = (0x6675_6e64, 0x7377_6570);

const SWEEPER_NAME: &str = "fundamental-schema-sweep";

/// Removes ephemeral schemas whose run is gone, and unmarked ones old enough that nobody can be
/// using them.
///
/// A held schema is reclaimed only when no session bears its name and its lock is free, and
/// only once the engine's locks are proved to exclude. An unmarked one keeps the age rule.
/// Pooled schemas are never touched. Drops and time are capped by [`SweepBudget`].
///
/// # Errors
/// The underlying [`SqlError`] if the database cannot be reached or the listing fails. A schema
/// that resists removal is logged and skipped; a sweep that runs out of time returns what it did.
#[instrument(skip_all, fields(drops = budget.drops, budget_ms = budget.time.as_millis()))]
pub async fn drop_stale_ephemeral_schemas(
    database_url: &str,
    minted_before_millis: u64,
    budget: SweepBudget,
) -> SqlResult<SweepOutcome> {
    // The budget covers the connection and its release too.
    let swept = timeout(budget.time, async {
        let mut sweeper = connect_dedicated(database_url, SWEEPER_NAME)
            .await
            .map_err(SqlError::Sqlx)?;
        let swept = sweep_on(&mut sweeper, database_url, minted_before_millis, budget).await;
        close_dedicated(sweeper).await;
        swept
    })
    .await;
    match swept {
        Ok(swept) => swept,
        Err(_) => Ok(SweepOutcome {
            timed_out: true,
            ..SweepOutcome::default()
        }),
    }
}

/// Sweeps the database on `sweeper`'s own session; releasing the locks it takes is the caller's.
///
/// # Errors
/// As [`drop_stale_ephemeral_schemas`], less the connection.
pub(super) async fn sweep_on(
    sweeper: &mut PgConnection,
    database_url: &str,
    minted_before_millis: u64,
    budget: SweepBudget,
) -> SqlResult<SweepOutcome> {
    let mut outcome = SweepOutcome::default();
    let swept = timeout(
        budget.time,
        sweep(
            sweeper,
            database_url,
            minted_before_millis,
            budget,
            &mut outcome,
        ),
    )
    .await;
    match swept {
        Ok(Ok(())) => Ok(outcome),
        Ok(Err(error)) => Err(error),
        Err(_) => {
            outcome.timed_out = true;
            Ok(outcome)
        }
    }
}

/// The sweep itself, recording into `outcome` as it goes so a timeout still reports the work done.
async fn sweep(
    sweeper: &mut PgConnection,
    database_url: &str,
    minted_before_millis: u64,
    budget: SweepBudget,
    outcome: &mut SweepOutcome,
) -> SqlResult<()> {
    if !try_advisory_lock(sweeper, SWEEPER_KEY)
        .await
        .map_err(SqlError::Sqlx)?
    {
        outcome.busy = true;
        return Ok(());
    }

    let held = held_schemas(sweeper).await?;
    let live = live_session_names(sweeper).await?;
    let candidates: Vec<&String> = held.iter().filter(|name| !live.contains(*name)).collect();
    outcome.live += held.len() - candidates.len();

    let exclusive = candidates.is_empty() || locks_exclude(database_url).await;
    reclaim_held(sweeper, &candidates, exclusive, budget, outcome).await;
    if outcome.capped {
        return Ok(());
    }

    let room = budget.drops.saturating_sub(outcome.reclaimed());
    let aged = aged_schemas(sweeper, minted_before_millis, room + 1).await?;
    for name in aged.iter().take(room) {
        if drop_schema(sweeper, name, budget).await {
            outcome.reclaimed_aged += 1;
        }
    }
    if aged.len() > room {
        outcome.capped = true;
    }
    Ok(())
}

/// Reclaims the held `candidates` whose lock is free; with `exclusive` false, nothing is dropped.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn reclaim_held(
    sweeper: &mut PgConnection,
    candidates: &[&String],
    exclusive: bool,
    budget: SweepBudget,
    outcome: &mut SweepOutcome,
) {
    if !exclusive {
        warn!(
            candidates = candidates.len(),
            "schema sweep: a held advisory lock did not exclude a second session, so liveness \
             cannot be read here; held schemas are left alone"
        );
        outcome.unknown += candidates.len();
        return;
    }
    for name in candidates {
        if outcome.reclaimed() >= budget.drops {
            outcome.capped = true;
            return;
        }
        let Some(key) = HeldName::advisory_key_of(name) else {
            outcome.unknown += 1;
            continue;
        };
        match try_advisory_lock(sweeper, key).await {
            Ok(true) => {
                if drop_schema(sweeper, name, budget).await {
                    outcome.reclaimed_dead += 1;
                }
            }
            Ok(false) => outcome.live += 1,
            Err(error) => {
                warn!(schema = %name, %error, "schema sweep: could not ask whether this schema is held; left alone");
                outcome.unknown += 1;
            }
        }
    }
}

async fn held_schemas(sweeper: &mut PgConnection) -> SqlResult<Vec<String>> {
    // The pattern only narrows; `HeldName::advisory_key_of` decides.
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT schema_name::text FROM information_schema.schemata \
         WHERE schema_name::text ~ $1 \
         ORDER BY substring(schema_name::text from $1)",
    )
    .bind(HeldName::SQL_PATTERN)
    .fetch_all(&mut *sweeper)
    .await
    .map_err(SqlError::Sqlx)?;
    Ok(names
        .into_iter()
        .filter(|name| HeldName::advisory_key_of(name).is_some())
        .collect())
}

async fn live_session_names(sweeper: &mut PgConnection) -> SqlResult<HashSet<String>> {
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT application_name::text FROM pg_stat_activity \
         WHERE application_name IS NOT NULL AND application_name <> ''",
    )
    .fetch_all(&mut *sweeper)
    .await
    .map_err(SqlError::Sqlx)?;
    Ok(names.into_iter().collect())
}

async fn aged_schemas(
    sweeper: &mut PgConnection,
    minted_before_millis: u64,
    limit: usize,
) -> SqlResult<Vec<String>> {
    // The fixed-width stamp compares as text; `::text`, as `schema_name` is the `sql_identifier` domain.
    sqlx::query_scalar(
        "SELECT schema_name::text FROM information_schema.schemata \
         WHERE schema_name::text ~ $1 \
           AND substring(schema_name::text from $1) < $2 \
         ORDER BY schema_name LIMIT $3",
    )
    .bind(EphemeralName::SQL_PATTERN)
    .bind(format!(
        "{minted_before_millis:0width$}",
        width = EphemeralName::STAMP_DIGITS
    ))
    .bind(i64::try_from(limit).unwrap_or(i64::MAX))
    .fetch_all(&mut *sweeper)
    .await
    .map_err(SqlError::Sqlx)
}

/// Whether a lock held by this sweep refuses a second session; `false` when it cannot open one.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn locks_exclude(database_url: &str) -> bool {
    let mut probe = match connect_dedicated(database_url, SWEEPER_NAME).await {
        Ok(probe) => probe,
        Err(error) => {
            warn!(%error, "schema sweep: could not open a second session to prove locks exclude");
            return false;
        }
    };
    let answer = try_advisory_lock(&mut probe, SWEEPER_KEY).await;
    close_dedicated(probe).await;
    match answer {
        Ok(taken) => !taken,
        Err(error) => {
            warn!(%error, "schema sweep: could not ask a second session for the sweeper's lock");
            false
        }
    }
}

/// Drops `name` on the sweep's own session and says whether it went; a failure is logged, not fatal.
async fn drop_schema(sweeper: &mut PgConnection, name: &str, budget: SweepBudget) -> bool {
    match drop_schema_on(sweeper, name, budget.time).await {
        Ok(()) => true,
        Err(error) => {
            warn!(schema = %name, %error, "schema sweep: DROP SCHEMA failed; the next sweep tries again");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_test_containers::sql::SqlDatabase;

    use super::*;
    use crate::test_support::schema_exists;
    use crate::{SqlConfig, SqlPool};

    async fn dead_schema(url: &str) -> (SqlPool, String) {
        let pool = SqlPool::connect(&SqlConfig::development(url))
            .await
            .expect("a pool on the test database");
        let name = HeldName::mint("not_exclusive_", 63)
            .expect("a held name")
            .to_string();
        pool.execute_ddl(&format!("CREATE SCHEMA IF NOT EXISTS {name}"))
            .await
            .expect("the schema is created");
        (pool, name)
    }

    #[tokio::test]
    async fn locks_that_do_not_exclude_drop_nothing_held() {
        let database = SqlDatabase::mint()
            .await
            .expect("a database of this test's own");
        let url = database.url().to_owned();
        let (pool, name) = dead_schema(&url).await;
        let mut sweeper = connect_dedicated(&url, SWEEPER_NAME)
            .await
            .expect("the sweeper connects");

        let mut refused = SweepOutcome::default();
        reclaim_held(
            &mut sweeper,
            &[&name],
            false,
            SweepBudget::ON_PROVISION,
            &mut refused,
        )
        .await;
        assert!(
            schema_exists(&pool, &name)
                .await
                .expect("the catalog answers"),
            "{name} was dropped on a lock that proved nothing: {refused:?}"
        );
        assert_eq!(refused.unknown, 1, "{refused:?}");
        assert_eq!(refused.reclaimed_dead, 0, "{refused:?}");

        let mut proved = SweepOutcome::default();
        reclaim_held(
            &mut sweeper,
            &[&name],
            true,
            SweepBudget::ON_PROVISION,
            &mut proved,
        )
        .await;
        assert_eq!(proved.reclaimed_dead, 1, "{proved:?}");
        assert!(
            !schema_exists(&pool, &name)
                .await
                .expect("the catalog answers")
        );
        close_dedicated(sweeper).await;
    }

    #[tokio::test]
    async fn the_pinned_engines_locks_are_proved_to_exclude() {
        let database = SqlDatabase::mint()
            .await
            .expect("a database of this test's own");
        let url = database.url().to_owned();
        let mut sweeper = connect_dedicated(&url, SWEEPER_NAME)
            .await
            .expect("the sweeper connects");
        assert!(
            try_advisory_lock(&mut sweeper, SWEEPER_KEY)
                .await
                .expect("the lock answers"),
            "nothing else sweeps this database"
        );
        assert!(
            locks_exclude(&url).await,
            "a held lock must refuse a second session"
        );
        close_dedicated(sweeper).await;
        assert!(
            !locks_exclude(&url).await,
            "with the sweeper's lock released the probe takes it, which is what \"not \
             exclusive\" would look like — so the probe can tell the two apart"
        );
    }
}
