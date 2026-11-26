//! [`EphemeralKeyspace`] — a throwaway, freshly-migrated YCQL keyspace.

use std::path::PathBuf;

use fundamental_cql::test_support::ReclaimedKeyspace;
use fundamental_cql::{CqlConfig, CqlIdent, CqlPool, ddl};
use fundamental_db::migration::{MigrationConfig, MigrationGuard};
use tracing::field::Empty;
use tracing::{Span, debug, instrument};

use super::EphemeralError;
use crate::MigrateRunner;

/// The replication a test keyspace is created with.
/// Test support only: a production keyspace is created by an operator with a real topology.
const TEST_REPLICATION: &str = "{'class': 'SimpleStrategy', 'replication_factor': 1}";

/// A uniquely-named YCQL keyspace, migrated on creation and dropped on [`Drop`].
///
/// Not `Debug`: it holds connection details.
///
/// ```no_run
/// // A doctest is its own crate, so the crate attribute has to be stated here: the
/// // provisioning future nests deeply enough (pool connect, DDL, migrate) that
/// // computing its layout exceeds rustc's default query depth.
/// #![recursion_limit = "256"]
/// use fundamental_cql::test_support::test_node_config;
/// use fundamental_db_migrator::EphemeralKeyspace;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let keyspace = EphemeralKeyspace::provision(
///     test_node_config(["127.0.0.1:9042"]),
///     "layer2_test",
///     "apps/layer2/migrations",
/// )
/// .await?;
/// let pool = keyspace.pool().clone();
/// # let _ = pool;
/// # Ok(())
/// # }
/// ```
pub struct EphemeralKeyspace {
    pool: CqlPool,
    /// The name, and the promise to remove whatever was created under it; its `Drop` is the
    /// teardown.
    reclaimed: ReclaimedKeyspace,
}

impl EphemeralKeyspace {
    /// Creates `<prefix>_<uuid>` on the cluster `config` reaches and applies the
    /// migrations in `migrations_dir`.
    ///
    /// `migrations_dir` is the app's `migrations` root; the runner appends the backend directory.
    /// # Errors
    /// Returns [`EphemeralError::Unreachable`] if the cluster cannot be contacted —
    /// the signal a suite turns into "skip" — and [`EphemeralError::InvalidPrefix`],
    /// [`Create`](EphemeralError::Create), [`Connect`](EphemeralError::Connect) or
    /// [`Migrate`](EphemeralError::Migrate) if the cluster answered but provisioning
    /// failed. Only the first is ever a reason to skip.
    #[instrument(skip_all, fields(prefix = prefix, name = Empty), err(Debug))]
    pub async fn provision(
        config: CqlConfig,
        prefix: &str,
        migrations_dir: impl Into<PathBuf>,
    ) -> Result<Self, EphemeralError> {
        // First, so every `?` below fails with the keyspace's name already held and reclaimed.
        let reclaimed = ReclaimedKeyspace::claim(config.clone(), prefix)?;
        let name = reclaimed.name().to_owned();
        Span::current().record("name", &name);

        // Swept on the way in, once per process: a `SIGKILL` runs no `Drop`.
        super::sweep_keyspaces_once(&config).await;

        let bootstrap =
            CqlPool::connect(&config)
                .await
                .map_err(|error| EphemeralError::Unreachable {
                    detail: error.to_string(),
                })?;

        // `name` came from `EphemeralName`, and `CqlIdent` re-checks it: CQL cannot
        // bind an identifier, so the guard is what makes interpolating one safe.
        let keyspace = CqlIdent::new(&name).map_err(|error| EphemeralError::Create {
            name: name.clone(),
            detail: error.to_string(),
        })?;
        bootstrap
            .execute_ddl(&ddl!(
                "CREATE KEYSPACE {} WITH replication = {TEST_REPLICATION}",
                keyspace
            ))
            .await
            .map_err(|error| EphemeralError::Create {
                name: name.clone(),
                detail: error.to_string(),
            })?;

        let pool = CqlPool::connect(&config.clone().keyspace(name.as_str()))
            .await
            .map_err(|error| EphemeralError::Connect {
                name: name.clone(),
                detail: error.to_string(),
            })?;

        MigrateRunner::new(
            MigrationConfig {
                migrations_dir: migrations_dir.into(),
                environment: "development".to_owned(),
                is_protected: false,
            },
            // A fixture applies whatever schema the test hands it; the guard's subject is a
            // fleet's own migrations, judged where they are applied for real.
            MigrationGuard::unenforced(),
        )
        .apply_cql(&pool)
        .await
        .map_err(|error| EphemeralError::Migrate {
            name: name.clone(),
            detail: error.to_string(),
        })?;

        debug!(name = %name, "ephemeral YCQL keyspace provisioned");
        Ok(Self { pool, reclaimed })
    }

    /// The pool for this keyspace. Clone it to hand to code under test.
    #[must_use]
    pub fn pool(&self) -> &CqlPool {
        &self.pool
    }

    /// The generated keyspace name, for diagnostics.
    #[must_use]
    pub fn name(&self) -> &str {
        self.reclaimed.name()
    }
}

#[cfg(test)]
mod tests {
    use fundamental_cql::test_support::test_node_config;

    use super::*;

    #[tokio::test]
    async fn an_unsafe_prefix_is_rejected_before_the_cluster_is_contacted() {
        // An unroutable host: the prefix check must fail closed before anything is contacted.
        let refused = matches!(
            EphemeralKeyspace::provision(
                test_node_config(["127.0.0.1:1"]),
                "ks; DROP KEYSPACE prod; --",
                "migrations",
            )
            .await,
            Err(EphemeralError::InvalidPrefix { .. })
        );
        assert!(
            refused,
            "an unsafe prefix must be refused before the cluster is contacted"
        );
    }
}
