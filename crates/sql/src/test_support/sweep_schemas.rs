use std::str::FromStr;
use std::time::Instant;

use fundamental_db::ephemeral::stale_cutoff_millis;
use sqlx::postgres::PgConnectOptions;
use tracing::{info, instrument, warn};

use super::{SweepBudget, SweepOutcome, drop_stale_ephemeral_schemas};
use crate::SqlResult;

/// Sweeps the database at `database_url` within `budget` and logs it, naming never the URL; never fails its caller.
#[instrument(skip_all, fields(drops = budget.drops, budget_ms = budget.time.as_millis()))]
pub async fn sweep_schemas(database_url: &str, budget: SweepBudget) -> Option<SweepOutcome> {
    let started = Instant::now();
    let swept = drop_stale_ephemeral_schemas(database_url, stale_cutoff_millis(), budget).await;
    log_sweep(database_url, swept, started)
}

/// Logs one sweep's outcome, naming the database and never the URL, and answers the outcome.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(super) fn log_sweep(
    database_url: &str,
    swept: SqlResult<SweepOutcome>,
    started: Instant,
) -> Option<SweepOutcome> {
    let database = PgConnectOptions::from_str(database_url)
        .ok()
        .and_then(|options| options.get_database().map(str::to_owned))
        .unwrap_or_default();
    match swept {
        Ok(outcome) => {
            info!(
                %database,
                reclaimed_dead = outcome.reclaimed_dead,
                reclaimed_aged = outcome.reclaimed_aged,
                live = outcome.live,
                unknown = outcome.unknown,
                busy = outcome.busy,
                capped = outcome.capped,
                timed_out = outcome.timed_out,
                elapsed_ms = started.elapsed().as_millis(),
                "schema sweep: done"
            );
            Some(outcome)
        }
        Err(error) => {
            warn!(
                %database, %error,
                "schema sweep: could not sweep; provisioning continues"
            );
            None
        }
    }
}
