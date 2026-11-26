use std::time::Duration;

use fundamental_db::ephemeral::EphemeralName;

use crate::{CqlConfig, CqlError, CqlPool, CqlResult};

/// How long the whole sweep may take before it stops and reports what it did.
const SWEEP_BUDGET: Duration = Duration::from_secs(60);

/// The most keyspaces one sweep will remove.
///
/// A cap keeps a long-idle machine from spending minutes on cleanup; the shortfall is reported.
const SWEEP_LIMIT: usize = 32;

/// Removes test keyspaces left behind by runs that never reached their teardown.
///
/// `system_schema.keyspaces` records no creation time, so a keyspace is removed only when its
/// name carries `prefix` and sorts before `minted_before`; the harness's fixed-width stamp makes
/// that order chronological.
///
/// Returns how many keyspaces were removed.
///
/// # Errors
/// Returns [`CqlError`] if the cluster cannot be reached or the listing fails. A
/// failure to drop an *individual* keyspace is not an error — it is reported on stderr,
/// named, and the sweep continues.
pub async fn drop_stale_test_keyspaces(
    config: &CqlConfig,
    prefix: &str,
    minted_before: &str,
) -> CqlResult<usize> {
    if !is_inlinable(prefix) || !is_inlinable(minted_before) {
        eprintln!("sweep: refusing to inline '{prefix}'/'{minted_before}'; nothing was swept");
        return Ok(0);
    }

    let pool = CqlPool::connect(config).await?;
    let swept = tokio::time::timeout(SWEEP_BUDGET, async {
        // CQL has no `LIKE` nor a range filter over a system table, so the predicate runs here.
        let mut stale: Vec<String> = pool
            .fetch_all::<(String,)>(&crate::cql!(
                "SELECT keyspace_name FROM system_schema.keyspaces"
            ))
            .await?
            .into_iter()
            .map(|(name,)| name)
            .filter(|name| name.starts_with(prefix) && name.as_str() < minted_before)
            .collect();
        stale.sort_unstable();

        let over_cap = stale.len().saturating_sub(SWEEP_LIMIT);
        let mut removed = 0;
        for name in stale.into_iter().take(SWEEP_LIMIT) {
            // The shared sequence, which drops the tables first.
            match super::drop_keyspace_with_tables(&pool, &name).await {
                Ok(()) => removed += 1,
                // Not fatal: another binary may have dropped it a moment ago.
                Err(error) => eprintln!("sweep: DROP KEYSPACE {name} failed: {error}"),
            }
        }
        if over_cap > 0 {
            eprintln!(
                "sweep: removed {removed} stale test keyspaces and stopped at the cap; \
                 {over_cap} remain and the next run will take them"
            );
        }
        Ok::<_, CqlError>(removed)
    })
    .await;

    match swept {
        Ok(result) => result,
        Err(_) => {
            eprintln!(
                "sweep: did not finish within {}s; stale test keyspaces remain",
                SWEEP_BUDGET.as_secs()
            );
            Ok(0)
        }
    }
}

/// Removes stale ephemeral keyspaces **under every prefix at once**.
///
/// It asks the grammar rather than a prefix list: every name
/// [`EphemeralName`](fundamental_db::ephemeral::EphemeralName) mints ends in
/// `_<13 digits>_<16 hex>`. CQL offers no live-session check, so the age bound stands alone.
///
/// Returns how many keyspaces were removed.
///
/// # Errors
/// Returns [`CqlError`] if the cluster cannot be reached or the listing fails. An
/// individual keyspace that resists removal is reported and skipped.
pub async fn drop_stale_ephemeral_keyspaces(
    config: &CqlConfig,
    minted_before_millis: u64,
) -> CqlResult<usize> {
    let pool = CqlPool::connect(config).await?;
    let swept = tokio::time::timeout(SWEEP_BUDGET, async {
        let mut stale: Vec<String> = pool
            .fetch_all::<(String,)>(&crate::cql!(
                "SELECT keyspace_name FROM system_schema.keyspaces"
            ))
            .await?
            .into_iter()
            .map(|(name,)| name)
            .filter(|name| {
                EphemeralName::stamp_of(name).is_some_and(|born| born < minted_before_millis)
            })
            .collect();
        stale.sort_unstable();

        let over_cap = stale.len().saturating_sub(SWEEP_LIMIT);
        let mut removed = 0;
        for name in stale.into_iter().take(SWEEP_LIMIT) {
            match super::drop_keyspace_with_tables(&pool, &name).await {
                Ok(()) => removed += 1,
                Err(error) => eprintln!("sweep: DROP KEYSPACE {name} failed: {error}"),
            }
        }
        if over_cap > 0 {
            eprintln!(
                "sweep: removed {removed} stale ephemeral keyspaces and stopped at the cap; \
                 {over_cap} remain and the next run will take them"
            );
        }
        Ok::<_, crate::CqlError>(removed)
    })
    .await;

    match swept {
        Ok(result) => result,
        Err(_) => {
            eprintln!(
                "sweep: did not finish within {}s; stale ephemeral keyspaces remain",
                SWEEP_BUDGET.as_secs()
            );
            Ok(0)
        }
    }
}

/// Whether `value` is safe to inline into CQL — plain identifier characters only.
fn is_inlinable(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_identifiers_are_inlinable() {
        for value in ["t_", "t_1754500000000_", "ks", "abc123"] {
            assert!(is_inlinable(value), "{value}");
        }
    }

    #[test]
    fn anything_that_could_end_the_statement_is_not() {
        for value in ["", "ks'", "ks; DROP KEYSPACE prod; --", "ks\"", "ks-1"] {
            assert!(!is_inlinable(value), "{value:?} must be refused");
        }
    }
}
