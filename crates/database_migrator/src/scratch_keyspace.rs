//! [`ScratchKeyspace`] — an empty keyspace beside an app's own, for one replay, removed after.

use std::time::{SystemTime, UNIX_EPOCH};

use fundamental_cql::{CqlIdent, CqlPool, ddl, drop_keyspace_with_tables};
use fundamental_db::DatabaseError;
use fundamental_db::migration::{MigrationError, MigrationResult};
use tracing::{debug, instrument, warn};

/// An empty keyspace on the cluster an app's pool reaches, with a pool of its own pinned to it,
/// so the unqualified statements of a migration land in it.
pub(crate) struct ScratchKeyspace {
    pool: CqlPool,
    name: String,
}

impl ScratchKeyspace {
    /// Makes `fundamental_scratch_<stamp>` beside `app`'s keyspace.
    ///
    /// # Errors
    /// The keyspace could not be made or connected to; one made is removed again.
    #[instrument(skip(app), err(Debug))]
    pub(crate) async fn create(app: &CqlPool) -> MigrationResult<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        // Short: a keyspace name is at most 48 characters.
        let name = format!("fundamental_scratch_{:x}", stamp & u128::from(u64::MAX));
        let keyspace =
            CqlIdent::new(&name).map_err(|error| MigrationError::Database(error.to_string()))?;
        app.execute_ddl(&ddl!("CREATE KEYSPACE IF NOT EXISTS {}", keyspace))
            .await
            .map_err(DatabaseError::from)?;
        match CqlPool::connect(&app.config().clone().keyspace(name.as_str())).await {
            Ok(pool) => {
                debug!(keyspace = %name, "scratch keyspace made");
                Ok(Self { pool, name })
            }
            Err(error) => {
                drop_keyspace(app, &name).await;
                Err(DatabaseError::from(error).into())
            }
        }
    }

    /// The pool whose statements land in this keyspace.
    pub(crate) fn pool(&self) -> &CqlPool {
        &self.pool
    }

    /// Removes the keyspace with everything in it, through `app`. A keyspace that could not be
    /// removed is logged with its name, to be dropped by hand.
    pub(crate) async fn remove(self, app: &CqlPool) {
        drop_keyspace(app, &self.name).await;
    }
}

/// [`drop_keyspace_with_tables`], logged when it fails.
#[expect(
    clippy::cognitive_complexity,
    reason = "straight-line; each tracing event's expansion is counted"
)]
async fn drop_keyspace(app: &CqlPool, name: &str) {
    if let Err(error) = drop_keyspace_with_tables(app, name).await {
        warn!(keyspace = %name, %error, "a scratch keyspace was left behind; drop it by hand");
        return;
    }
    debug!(keyspace = %name, "scratch keyspace removed");
}
