//! A schema with the lease tables, and leases short enough to expire inside a test.

// Each suite compiles this module into its own binary and uses a different part of it.
#![allow(dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::time::Duration;

use fundamental_db::migration::{Lints, MigrationConfig, MigrationGuard};
use fundamental_db_migrator::MigrateRunner;
use fundamental_lease::{LEASE_MIGRATIONS, Lease, LeaseConfig, LeaseName, LeaseSettings};
use fundamental_sql::SqlPool;
use fundamental_test_containers::sql::SqlSchema;

/// A fresh schema with the lease tables applied through the embedded migrations.
pub async fn schema_with_leases() -> SqlSchema {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let dir = tempfile::tempdir().expect("a temporary directory");
    MigrateRunner::new(
        MigrationConfig {
            migrations_dir: dir.path().to_path_buf(),
            environment: "test".to_owned(),
            is_protected: false,
        },
        MigrationGuard::new(1_000, Lints::default(), &[]),
    )
    .with_embedded(&LEASE_MIGRATIONS)
    .apply_sql(schema.pool())
    .await
    .expect("the lease tables apply");
    schema
}

/// Two-second leases, renewed four times a TTL, a quarter-second drain.
pub fn quick() -> LeaseConfig {
    LeaseConfig::new(LeaseSettings {
        ttl_secs: 2,
        renewals_per_ttl: 4,
        drain_grace_ms: 250,
        max_clock_skew_ms: 100,
        work_retention_hours: 1,
    })
    .expect("valid timings")
}

pub fn lease(pool: &SqlPool, name: &'static str) -> Lease {
    Lease::new(
        pool.clone(),
        LeaseName::new(name).expect("a lease name"),
        quick(),
    )
}

/// Long enough for a two-second lease to expire by the database's clock.
pub const PAST_TTL: Duration = Duration::from_millis(2_300);
