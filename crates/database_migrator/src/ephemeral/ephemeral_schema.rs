//! [`EphemeralSchema`] — a throwaway, freshly-migrated schema inside an app's own database.

use std::path::PathBuf;
use std::time::Duration;

use fundamental_db::SchemaName;
use fundamental_db::migration::{MigrationConfig, MigrationGuard};
use fundamental_sql::test_support::ReclaimedSchema;
use fundamental_sql::{SqlConfig, SqlPool};
use tracing::field::Empty;
use tracing::{Span, debug, instrument, warn};

use super::EphemeralError;
use crate::MigrateRunner;

/// How many times a whole provisioning, each with a fresh name, is attempted before its failure
/// reaches the caller. Retried whatever the failure: a scratch schema is disposable.
const PROVISION_ATTEMPTS: u32 = 3;

/// How long to wait after a failed provisioning before a fresh one, doubled each time: it waits
/// out several fixtures migrating into one database at once.
const PROVISION_BACKOFF: Duration = Duration::from_millis(750);

/// A uniquely-named schema, migrated on creation and dropped on [`Drop`].
///
/// Hold the value for the whole test: the schema is dropped with it.
///
/// Not `Debug`: it holds a connection URL, which carries credentials.
///
/// ```no_run
/// use fundamental_db_migrator::EphemeralSchema;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # use fundamental_db::migration::MigrationGuard;
/// let db = EphemeralSchema::provision(
///     "postgresql://layer2@localhost:5433/layer2?sslmode=disable",
///     "layer2_test_",
///     "apps/layer2/migrations",
///     MigrationGuard::unenforced(),
/// )
/// .await?;
/// let pool = db.pool().clone();
/// # let _ = pool;
/// # Ok(())
/// # }
/// ```
pub struct EphemeralSchema {
    /// The pool addressing this schema.
    ///
    /// First field: fields drop in order, so the pool closes before `reclaimed` drops the schema.
    pool: SqlPool,
    url: String,
    schema: SchemaName,
    /// The name, and the promise to remove whatever was created under it; its `Drop` is the
    /// teardown.
    reclaimed: ReclaimedSchema,
}

impl EphemeralSchema {
    /// Creates `<prefix><stamp>_<random>` in the database behind `database_url` and applies
    /// the migrations in `migrations_dir` into it.
    ///
    /// `database_url` addresses the app's own database, as the app's own role; `migrations_dir` is
    /// the app's `migrations` root. `guard` is stated rather than defaulted, so the app's shipped
    /// migrations are judged as the app judges them.
    ///
    /// # Errors
    /// Returns [`EphemeralError::Unreachable`] if the database cannot be contacted — the
    /// signal a suite turns into "skip" — and [`InvalidPrefix`](EphemeralError::InvalidPrefix),
    /// [`Create`](EphemeralError::Create) or [`Migrate`](EphemeralError::Migrate) if it
    /// answered but provisioning failed. Only the first is ever a reason to skip.
    #[instrument(skip_all, fields(prefix = prefix, name = Empty, attempts = Empty), err(Debug))]
    pub async fn provision(
        database_url: &str,
        prefix: &str,
        migrations_dir: impl Into<PathBuf>,
        guard: MigrationGuard,
    ) -> Result<Self, EphemeralError> {
        let migrations_dir = migrations_dir.into();

        // Swept on the way in by this process's first claim in this database; a retry's claim does
        // not sweep again.

        let mut backoff = PROVISION_BACKOFF;
        for attempt in 1..=PROVISION_ATTEMPTS {
            let outcome =
                Self::provision_once(database_url, prefix, migrations_dir.clone(), guard.clone())
                    .await;
            let error = match outcome {
                Ok(fixture) => {
                    Span::current().record("attempts", attempt);
                    if attempt > 1 {
                        debug!(
                            attempt,
                            name = fixture.name(),
                            "schema provisioned after a retry"
                        );
                    }
                    return Ok(fixture);
                }
                Err(error) => error,
            };

            // `Unreachable` is the skip signal and `InvalidPrefix` answers the same way forever:
            // neither is retried.
            let hopeless =
                error.is_unreachable() || matches!(error, EphemeralError::InvalidPrefix { .. });
            if hopeless || attempt == PROVISION_ATTEMPTS {
                return Err(error);
            }

            warn!(
                attempt,
                of = PROVISION_ATTEMPTS,
                backoff_ms = backoff.as_millis(),
                error = %error,
                "provisioning a scratch schema failed; the half-built one is reclaimed and a \
                 fresh name is minted"
            );
            tokio::time::sleep(backoff).await;
            backoff *= 2;
        }

        // Reached only with a `PROVISION_ATTEMPTS` of zero.
        Err(EphemeralError::Create {
            name: prefix.to_owned(),
            detail: "no provisioning attempt was made".to_owned(),
        })
    }

    /// One claim/create/verify/migrate cycle of [`provision`](Self::provision); a failure drops the
    /// guard, which reclaims.
    async fn provision_once(
        database_url: &str,
        prefix: &str,
        migrations_dir: PathBuf,
        guard: MigrationGuard,
    ) -> Result<Self, EphemeralError> {
        // First, before the database is contacted: every `?` below may follow a created schema,
        // which the guard reclaims.
        let reclaimed = ReclaimedSchema::claim(database_url, prefix).await?;
        let name = reclaimed.name().to_owned();
        Span::current().record("name", &name);

        // Converted rather than assumed: this value is interpolated into DDL.
        let schema = SchemaName::new(name.clone()).map_err(|error| EphemeralError::Create {
            name: name.clone(),
            detail: format!("minted name is not a schema identifier: {error}"),
        })?;

        // One pool, also the reachability probe: `search_path` resolves per statement, so it may
        // name the schema created below.
        let pool = SqlPool::connect(
            &SqlConfig::development(database_url)
                .default_schema(schema.clone())
                // Every connection bears the schema's name: the liveness signal that survives
                // the engine ending the holder's session while this test is still connected.
                .application_name(name.as_str()),
        )
        .await
        .map_err(|error| EphemeralError::Unreachable {
            detail: error.to_string(),
        })?;

        // `execute_ddl`: DDL contends on a catalog latch, and the statement is idempotent.
        pool.execute_ddl(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .await
            .map_err(|error| EphemeralError::Create {
                name: name.clone(),
                detail: error.to_string(),
            })?;

        // Fail closed: `SET search_path` to a missing schema does not error, so ask the catalog.
        let exists = pool
            .fetch_optional(&format!(
                "SELECT 1 FROM information_schema.schemata WHERE schema_name = '{schema}'"
            ))
            .await
            .map_err(|error| EphemeralError::Create {
                name: name.clone(),
                detail: format!("could not confirm the schema exists: {error}"),
            })?;
        if exists.is_none() {
            return Err(EphemeralError::Create {
                name: name.clone(),
                detail: "the CREATE reported success and the catalog does not have it".to_owned(),
            });
        }

        MigrateRunner::new(
            MigrationConfig {
                migrations_dir,
                environment: "development".to_owned(),
                is_protected: false,
            },
            guard,
        )
        .apply_sql(&pool)
        .await
        .map_err(|error| EphemeralError::Migrate {
            name: name.clone(),
            detail: error.to_string(),
        })?;

        debug!(name = %name, "ephemeral schema provisioned");
        Ok(Self {
            pool,
            url: database_url.to_owned(),
            schema,
            reclaimed,
        })
    }

    /// The pool for this schema. Clone it to hand to code under test.
    #[must_use]
    pub fn pool(&self) -> &SqlPool {
        &self.pool
    }

    /// The generated schema name, for diagnostics and for DDL that has to qualify it.
    #[must_use]
    pub fn name(&self) -> &str {
        self.reclaimed.name()
    }

    /// The schema this fixture owns, for a caller that builds its own pool against it.
    #[must_use]
    pub fn schema(&self) -> &SchemaName {
        &self.schema
    }

    /// The connection URL of the database this schema lives in, for code that takes a URL
    /// rather than a pool.
    ///
    /// A caller building its own pool from it must also apply [`schema`](Self::schema), or it
    /// addresses `public`.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_unsafe_prefix_is_rejected_before_the_database_is_contacted() {
        // An unroutable address: the prefix check must fail closed before anything is contacted.
        let refused = matches!(
            EphemeralSchema::provision(
                "postgresql://yugabyte@127.0.0.1:1/base?sslmode=disable",
                "x; DROP SCHEMA prod; --",
                "migrations",
                MigrationGuard::unenforced(),
            )
            .await,
            Err(EphemeralError::InvalidPrefix { .. })
        );
        assert!(
            refused,
            "an unsafe prefix must be refused before the database is contacted"
        );
    }

    /// A held claim's failure to connect is still the skip signal, never a provisioning failure.
    #[tokio::test]
    async fn an_unreachable_database_is_still_a_skip() {
        let error = EphemeralSchema::provision(
            "postgresql://yugabyte@127.0.0.1:1/base?sslmode=disable",
            "layer5_test_",
            "migrations",
            MigrationGuard::unenforced(),
        )
        .await
        .err()
        .expect("nothing listens there");
        assert!(error.is_unreachable(), "{error:?}");
    }
}
