use std::time::{Duration, Instant};

use sqlx::postgres::PgConnection;
use tokio::time::{sleep, timeout};
use tracing::debug;

use super::is_inlinable;
use crate::SqlError;

const FIRST_PAUSE: Duration = Duration::from_millis(100);

const MAX_PAUSE: Duration = Duration::from_secs(2);

/// Drops the schema `name` on a connection the caller already has, retrying what the engine says
/// is worth retrying for at most `budget`; `IF EXISTS` makes a retry safe.
///
/// # Errors
/// [`SqlError::Query`] if `name` cannot be inlined into DDL or the budget is spent mid-statement;
/// the connection's own failure at once if its session is gone; otherwise the engine's last
/// refusal, classified, once it is not worth retrying.
pub(super) async fn drop_schema_on(
    connection: &mut PgConnection,
    name: &str,
    budget: Duration,
) -> Result<(), SqlError> {
    if !is_inlinable(name) {
        return Err(SqlError::Query(format!(
            "refusing to inline `{name}` into DDL"
        )));
    }
    let statement = format!("DROP SCHEMA IF EXISTS {name} CASCADE");
    let deadline = Instant::now() + budget;
    let mut pause = FIRST_PAUSE;
    let mut attempt = 1_u32;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let answered = timeout(
            remaining,
            sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(&mut *connection),
        )
        .await;
        let error = match answered {
            Ok(Ok(_)) => return Ok(()),
            // A session that is gone never comes back; the caller takes a fresh connection.
            Ok(Err(error @ (sqlx::Error::Io(_) | sqlx::Error::Protocol(_)))) => {
                return Err(SqlError::from(error));
            }
            Ok(Err(error)) => SqlError::from(error),
            Err(_) => {
                return Err(SqlError::Query(format!(
                    "DROP SCHEMA {name} did not finish within {budget:?}"
                )));
            }
        };
        if !error.should_retry_in_process() || Instant::now() + pause >= deadline {
            return Err(error);
        }
        debug!(schema = %name, attempt, %error, "test support: DROP SCHEMA lost a conflict; running it again");
        sleep(pause).await;
        pause = (pause * 2).min(MAX_PAUSE);
        attempt += 1;
    }
}
