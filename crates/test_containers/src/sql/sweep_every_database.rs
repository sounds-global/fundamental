use std::time::Instant;

use fundamental_sql::sqlx::Row;
use fundamental_sql::test_support::{SweepBudget, sweep_schemas};
use fundamental_sql::{SqlConfig, SqlPool};
use tracing::{debug, info, warn};

use crate::sql::sql_node::SqlNode;
use crate::sql::sql_schema::HOST_PREFIX;

/// Sweeps the ephemeral schemas of every host database on the node, once per run.
///
/// Hosts only: a scratch is emptied by its next holder and a `t_` database by the database
/// sweep. Never fails the caller.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(super) async fn sweep_every_database(node: &SqlNode) {
    let bootstrap = node.bootstrap_url();
    let admin = match SqlPool::connect(&SqlConfig::development(bootstrap.as_str())).await {
        Ok(admin) => admin,
        Err(error) => {
            warn!(%error, "test containers: could not list host databases to sweep their schemas");
            return;
        }
    };
    let rows = match admin
        .fetch_all(&format!(
            "SELECT datname::text FROM pg_database \
             WHERE datallowconn AND NOT datistemplate AND datname::text LIKE '{HOST_PREFIX}%' \
             ORDER BY datname"
        ))
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            warn!(%error, "test containers: could not list host databases to sweep their schemas");
            return;
        }
    };

    let deadline = Instant::now() + SweepBudget::ONCE_PER_RUN.time;
    let mut drops_left = SweepBudget::ONCE_PER_RUN.drops;
    for row in &rows {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || drops_left == 0 {
            info!(
                drops_left,
                remaining_ms = remaining.as_millis(),
                "test containers: the run's schema sweep is spent; the fixtures' own sweeps take the rest"
            );
            return;
        }
        let database: String = match row.try_get(0) {
            Ok(database) => database,
            Err(error) => {
                warn!(%error, "test containers: a database name did not decode; skipped");
                continue;
            }
        };
        debug!(%database, "test containers: sweeping schemas, once per run");
        let budget = SweepBudget {
            drops: drops_left,
            time: remaining,
        };
        if let Some(outcome) = sweep_schemas(&node.url(&database), budget).await {
            drops_left = drops_left.saturating_sub(outcome.reclaimed());
        }
    }
}
