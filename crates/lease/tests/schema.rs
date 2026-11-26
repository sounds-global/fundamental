//! The shipped lease tables match their models, and nothing reads the wall clock. Partly requires Docker.

mod support;

use std::path::Path;

use fundamental_db::migration::{Lints, MigrationConfig, MigrationGuard};
use fundamental_db_migrator::MigrateRunner;
use fundamental_lease::LEASE_MIGRATIONS;

#[tokio::test]
async fn the_shipped_ddl_matches_the_model() {
    let schema = support::schema_with_leases().await;
    let dir = tempfile::tempdir().expect("a temporary directory");
    let check = MigrateRunner::new(
        MigrationConfig {
            migrations_dir: dir.path().to_path_buf(),
            environment: "test".to_owned(),
            is_protected: false,
        },
        MigrationGuard::new(1_000, Lints::default(), &[]),
    )
    .with_embedded(&LEASE_MIGRATIONS)
    .check_sql(schema.pool())
    .await
    .expect("nothing unapplied");
    let lease_drift: Vec<_> = check
        .drift
        .iter()
        .filter(|drift| format!("{drift:?}").contains("fundamental_lease"))
        .collect();
    assert!(lease_drift.is_empty(), "{lease_drift:?}");
}

#[test]
fn the_crate_never_reads_the_process_wall_clock() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&src).expect("the sources") {
        let path = entry.expect("an entry").path();
        let text = std::fs::read_to_string(&path).expect("a source file");
        for needle in ["Utc::now", "Local::now", "SystemTime::now", "chrono::Local"] {
            if text.contains(needle) {
                offenders.push(format!("{}: {needle}", path.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "an expiry judged by a process clock: {offenders:?}"
    );
}
