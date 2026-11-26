//! What the SQL-store suites share: a tuple table built the way a deployment builds it.

#![allow(dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use fundamental_authz::SQL_TUPLE_MIGRATIONS;
use fundamental_db::migration::{Lints, MigrationConfig, MigrationGuard};
use fundamental_db_migrator::MigrateRunner;
use fundamental_sql::SqlPool;

/// The migrator an app registers the set with, with a production guard.
pub fn runner(dir: &tempfile::TempDir) -> MigrateRunner {
    MigrateRunner::new(
        MigrationConfig {
            migrations_dir: dir.path().to_path_buf(),
            environment: "test".to_owned(),
            is_protected: false,
        },
        MigrationGuard::new(1_000, Lints::default(), &[]),
    )
    .with_embedded(&SQL_TUPLE_MIGRATIONS)
}

/// Applies `SQL_TUPLE_MIGRATIONS` to `pool`, returning the ledger names it applied.
pub async fn migrate_tuple_table(pool: &SqlPool) -> Vec<String> {
    let dir = tempfile::tempdir().expect("a temporary directory");
    // Boxed: the migrator's future overflows the compiler's layout query depth inline.
    Box::pin(runner(&dir).apply_sql(pool))
        .await
        .expect("the tuple table migrates")
}
