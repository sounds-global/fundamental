//! The inbox tables the crate ships match the models it reads them through, and nothing in it
//! reads the process's wall clock. The first half requires Docker.

mod support;

use std::path::Path;

use fundamental_db::migration::{Lints, MigrationConfig, MigrationGuard};
use fundamental_db_migrator::MigrateRunner;
use fundamental_inbox::INBOX_MIGRATIONS;

#[tokio::test]
async fn the_shipped_ddl_matches_the_models() {
    let schema = support::schema_with_inbox().await;
    let dir = tempfile::tempdir().expect("a temporary directory");
    let check = MigrateRunner::new(
        MigrationConfig {
            migrations_dir: dir.path().to_path_buf(),
            environment: "test".to_owned(),
            is_protected: false,
        },
        MigrationGuard::new(1_000, Lints::default(), &[]),
    )
    .with_embedded(&INBOX_MIGRATIONS)
    .check_sql(schema.pool())
    .await
    .expect("nothing unapplied");
    let drift: Vec<_> = check
        .drift
        .iter()
        .filter(|drift| format!("{drift:?}").contains("fundamental_inbox"))
        .collect();
    assert!(drift.is_empty(), "{drift:?}");
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
        "a window judged by a process clock: {offenders:?}"
    );
}
