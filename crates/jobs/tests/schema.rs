//! The job table the crate ships matches the model it reads it through, and nothing in the crate
//! judges time by the process's wall clock. The first half requires Docker.

mod support;

use std::path::Path;

use fundamental_db::migration::{Lints, MigrationConfig, MigrationGuard};
use fundamental_db_migrator::MigrateRunner;
use fundamental_jobs::JOBS_MIGRATIONS;

#[tokio::test]
async fn the_shipped_ddl_matches_the_model() {
    let (schema, _pool) = support::schema_with_jobs().await;
    let dir = tempfile::tempdir().expect("a temporary directory");
    let check = MigrateRunner::new(
        MigrationConfig {
            migrations_dir: dir.path().to_path_buf(),
            environment: "test".to_owned(),
            is_protected: false,
        },
        MigrationGuard::new(1_000, Lints::default(), &[]),
    )
    .with_embedded(&JOBS_MIGRATIONS)
    .check_sql(schema.pool())
    .await
    .expect("nothing unapplied");
    let job_drift: Vec<_> = check
        .drift
        .iter()
        .filter(|drift| format!("{drift:?}").contains("fundamental_jobs"))
        .collect();
    assert!(job_drift.is_empty(), "{job_drift:?}");
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
        "a due time judged by a process clock: {offenders:?}"
    );
}
