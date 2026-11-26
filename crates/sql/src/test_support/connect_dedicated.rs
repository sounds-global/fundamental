use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgConnection};
use sqlx::{ConnectOptions, Connection};
use tokio::time::timeout;
use tracing::{debug, warn};

/// How long releasing and closing a dedicated connection may take.
const CLOSE_BUDGET: Duration = Duration::from_secs(5);

/// Opens one connection, never a pool, to `database_url` as `application_name`, so a session advisory lock lives exactly as long as it.
///
/// # Errors
/// The underlying `sqlx` error if the URL does not parse or the connection is refused.
pub(super) async fn connect_dedicated(
    database_url: &str,
    application_name: &str,
) -> Result<PgConnection, sqlx::Error> {
    let options = PgConnectOptions::from_str(database_url)?
        .application_name(application_name)
        .disable_statement_logging();
    PgConnection::connect_with(&options).await
}

/// Releases every advisory lock `connection` holds, then closes it, within [`CLOSE_BUDGET`].
///
/// Released explicitly: on the pinned engine a cleanly closed session's locks are not free at once.
pub(super) async fn close_dedicated(mut connection: PgConnection) {
    let closed = timeout(CLOSE_BUDGET, async {
        if let Err(error) = sqlx::query("SELECT pg_advisory_unlock_all()")
            .execute(&mut connection)
            .await
        {
            debug!(%error, "test support: a dedicated connection could not release its locks; the engine releases them when the session ends");
        }
        if let Err(error) = connection.close().await {
            debug!(%error, "test support: a dedicated connection did not close cleanly; the engine releases it when the socket goes");
        }
    })
    .await;
    if closed.is_err() {
        warn!(
            budget_ms = CLOSE_BUDGET.as_millis(),
            "test support: a dedicated connection did not close in time; its socket is dropped and the engine releases its locks when it notices"
        );
    }
}
