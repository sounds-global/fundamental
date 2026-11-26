use std::time::Duration;

use sqlx::Row;

use super::is_inlinable;
use crate::{SqlConfig, SqlPool, SqlResult};

/// How long the whole sweep may take, so a wedged node does not stop a suite starting silently.
const SWEEP_BUDGET: Duration = Duration::from_secs(60);

/// The most databases one sweep will remove; the rest go next run.
const SWEEP_LIMIT: usize = 32;

/// Removes test databases under `prefix` minted before `minted_before`, left by runs that never
/// reached their teardown; returns how many were removed.
///
/// # Errors
/// Returns the underlying [`SqlError`](crate::SqlError) if the node cannot be
/// reached or the listing fails. A database that cannot be dropped is reported on stderr and skipped.
///
/// # Panics
/// Never: `prefix` and `minted_before` are refused unless they are plain identifier characters.
pub async fn drop_stale_test_databases(
    bootstrap_url: &str,
    prefix: &str,
    minted_before: &str,
) -> SqlResult<usize> {
    if !is_inlinable(prefix) || !is_inlinable(minted_before) {
        eprintln!(
            "sweep: refusing to inline '{prefix}'/'{minted_before}'; \
             no databases were swept"
        );
        return Ok(0);
    }

    let pool = SqlPool::connect(&SqlConfig::development(bootstrap_url)).await?;
    let swept = tokio::time::timeout(SWEEP_BUDGET, async {
        let stale = pool
            .fetch_all(&format!(
                "SELECT datname FROM pg_database \
                 WHERE datname LIKE '{prefix}%' AND datname < '{minted_before}' \
                 ORDER BY datname LIMIT {SWEEP_LIMIT}"
            ))
            .await?;

        let mut removed = 0;
        for row in &stale {
            let name: String = row.get(0);
            match pool
                // `execute_ddl`, since a drop loses to `40001` like any catalog write; `IF EXISTS` makes the retry safe.
                .execute_ddl(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
                .await
            {
                Ok(_) => removed += 1,
                Err(error) => eprintln!("sweep: DROP {name} failed: {error}"),
            }
        }
        if stale.len() == SWEEP_LIMIT {
            eprintln!(
                "sweep: removed {SWEEP_LIMIT} stale test databases and stopped at the cap; \
                 more remain and the next run will take them"
            );
        }
        Ok::<_, crate::SqlError>(removed)
    })
    .await;

    match swept {
        Ok(result) => result,
        Err(_) => {
            eprintln!(
                "sweep: did not finish within {}s; stale test databases remain",
                SWEEP_BUDGET.as_secs()
            );
            Ok(0)
        }
    }
}

/// Removes stale ephemeral databases under every prefix, by the `EphemeralName` grammar and its
/// stamp, leaving any database with a backend attached; returns how many were removed.
///
/// # Errors
/// Returns the underlying [`SqlError`](crate::SqlError) if the node cannot be reached or
/// the listing fails. An individual database that resists removal is reported and skipped.
///
/// # Panics
/// Never: every name is re-checked with `is_inlinable` before the DROP inlines it.
pub async fn drop_stale_ephemeral_databases(
    bootstrap_url: &str,
    minted_before_millis: u64,
) -> SqlResult<usize> {
    let pool = SqlPool::connect(&SqlConfig::development(bootstrap_url)).await?;
    let swept = tokio::time::timeout(SWEEP_BUDGET, async {
        // `{{13}}` and `{{16}}` are escaped regex braces; the fixed-width stamp compares as text.
        let stale = pool
            .fetch_all(&format!(
                "SELECT d.datname FROM pg_database d \
                 WHERE d.datname ~ '_[0-9]{{13}}_[0-9a-f]{{16}}$' \
                   AND substring(d.datname from '_([0-9]{{13}})_[0-9a-f]{{16}}$') \
                       < '{minted_before_millis:013}' \
                   AND NOT EXISTS ( \
                       SELECT 1 FROM pg_stat_activity a WHERE a.datid = d.oid) \
                 ORDER BY d.datname LIMIT {SWEEP_LIMIT}"
            ))
            .await?;

        let mut removed = 0;
        for row in &stale {
            let name: String = row.get(0);
            if !is_inlinable(&name) {
                eprintln!("sweep: refusing to inline '{name}'; it was left alone");
                continue;
            }
            match pool
                // `execute_ddl`, since a drop loses to `40001` like any catalog write; `IF EXISTS` makes the retry safe.
                .execute_ddl(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
                .await
            {
                Ok(_) => removed += 1,
                Err(error) => eprintln!("sweep: DROP {name} failed: {error}"),
            }
        }
        if stale.len() == SWEEP_LIMIT {
            eprintln!(
                "sweep: removed {SWEEP_LIMIT} stale ephemeral databases and stopped at the \
                 cap; more remain and the next run will take them"
            );
        }
        Ok::<_, crate::SqlError>(removed)
    })
    .await;

    match swept {
        Ok(result) => result,
        Err(_) => {
            eprintln!(
                "sweep: did not finish within {}s; stale ephemeral databases remain",
                SWEEP_BUDGET.as_secs()
            );
            Ok(0)
        }
    }
}
