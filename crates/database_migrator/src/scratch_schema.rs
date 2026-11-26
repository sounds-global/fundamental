//! [`ScratchSchema`] — an empty schema beside an app's own, for one replay, removed after.

use std::time::{SystemTime, UNIX_EPOCH};

use fundamental_db::migration::{MigrationError, MigrationResult};
use fundamental_db::{DatabaseError, SchemaName};
use fundamental_sql::SqlPool;
use tracing::{debug, instrument, warn};

/// An empty schema in the database an app's pool reaches, with a pool of its own whose search
/// path holds it alone, so the unqualified statements of a migration land in it.
///
/// Made with the app's own pool and configuration, so the replay connects as the app's migrator
/// does.
pub(crate) struct ScratchSchema {
    pool: SqlPool,
    name: SchemaName,
}

impl ScratchSchema {
    /// Makes `fundamental_verify_<purpose>_<stamp>` beside `app`'s schema.
    ///
    /// # Errors
    /// The schema could not be made or connected to; one made is removed again.
    #[instrument(skip(app), err(Debug))]
    pub(crate) async fn create(app: &SqlPool, purpose: &str) -> MigrationResult<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let name = SchemaName::new(format!(
            "fundamental_verify_{purpose}_{stamp:x}_{:x}",
            std::process::id()
        ))
        .map_err(|error| MigrationError::Database(error.to_string()))?;
        app.execute_ddl(&format!("CREATE SCHEMA IF NOT EXISTS {name}"))
            .await
            .map_err(DatabaseError::from)?;
        let config = app
            .config()
            .clone()
            .default_schema(name.clone())
            .application_name(name.as_str());
        match SqlPool::connect(&config).await {
            Ok(pool) => {
                debug!(schema = %name, "scratch schema made");
                Ok(Self { pool, name })
            }
            Err(error) => {
                drop_schema(app, &name).await;
                Err(DatabaseError::from(error).into())
            }
        }
    }

    /// The pool whose statements land in this schema.
    pub(crate) fn pool(&self) -> &SqlPool {
        &self.pool
    }

    /// Closes its pool and removes the schema with everything in it, through `app`. A schema
    /// that could not be removed is logged with its name, to be dropped by hand.
    pub(crate) async fn remove(self, app: &SqlPool) {
        self.pool.inner().close().await;
        drop_schema(app, &self.name).await;
    }
}

/// `DROP SCHEMA … CASCADE`, logged when it fails.
#[expect(
    clippy::cognitive_complexity,
    reason = "straight-line; each tracing event's expansion is counted"
)]
async fn drop_schema(app: &SqlPool, name: &SchemaName) {
    let dropped = app
        .execute_ddl(&format!("DROP SCHEMA IF EXISTS {name} CASCADE"))
        .await;
    if let Err(error) = dropped {
        warn!(schema = %name, %error, "a scratch schema was left behind; drop it by hand");
        return;
    }
    debug!(schema = %name, "scratch schema removed");
}
