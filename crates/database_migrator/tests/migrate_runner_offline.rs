//! End-to-end test of [`MigrateRunner`] against a live in-memory SQLite database
//! (no Docker): applying a hand-written migration, confirming the runner is
//! idempotent, and confirming the on-disk migrations replay into the schema the
//! models expect.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;

use fundamental_db::migration::{MigrationConfig, MigrationGuard};
use fundamental_db_migrator::MigrateRunner;
use fundamental_macros::Model;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
use tempfile::TempDir;

#[derive(Clone, Debug, Model)]
#[model(offline)]
struct Gadget {
    #[model(id)]
    id: String,
    count: i64,
    note: Option<String>,
}

fn config(dir: &Path) -> MigrationConfig {
    MigrationConfig {
        migrations_dir: dir.to_path_buf(),
        environment: "test".to_string(),
        is_protected: false,
    }
}

/// Writes `body` as the one offline migration on disk.
/// Hand-written: deriving it from the model would make the replay check assert nothing.
fn write_offline_migration(dir: &Path, name: &str, body: &str) {
    let engine_dir = dir.join("offline");
    std::fs::create_dir_all(&engine_dir).expect("migration dir");
    std::fs::write(engine_dir.join(format!("{name}.up.sql")), body).expect("write migration");
}

/// The table `Gadget` expects, written by hand.
/// `gadget`, not `gadgets`: the model declares no `table = …`.
const GADGET_TABLE: &str = "CREATE TABLE IF NOT EXISTS gadget (\
     id TEXT PRIMARY KEY NOT NULL, count INTEGER NOT NULL, note TEXT);";

#[tokio::test]
async fn applying_a_hand_written_migration_is_idempotent() {
    let tmp = TempDir::new().expect("temp dir");
    let runner = MigrateRunner::new(config(tmp.path()), MigrationGuard::unenforced())
        .offline_model::<Gadget>();
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("in-memory db");

    write_offline_migration(tmp.path(), "0001_create_gadget", GADGET_TABLE);

    let applied = runner.apply_offline(&db).await.expect("apply succeeds");
    assert_eq!(applied.len(), 1, "exactly one migration applied");

    let again_applied = runner.apply_offline(&db).await.expect("re-apply succeeds");
    assert!(
        again_applied.is_empty(),
        "already-applied migration is skipped"
    );
}

#[tokio::test]
async fn replay_check_passes_when_migrations_match_models() {
    let tmp = TempDir::new().expect("temp dir");
    let runner = MigrateRunner::new(config(tmp.path()), MigrationGuard::unenforced())
        .offline_model::<Gadget>();

    write_offline_migration(tmp.path(), "0001_create_gadget", GADGET_TABLE);

    runner
        .assert_offline_schema_matches_migrations()
        .await
        .expect("on-disk migrations reproduce the model schema");
}

#[tokio::test]
async fn replay_check_fails_when_a_migration_is_missing() {
    // A model is registered but no migration was ever generated.
    let tmp = TempDir::new().expect("temp dir");
    let runner = MigrateRunner::new(config(tmp.path()), MigrationGuard::unenforced())
        .offline_model::<Gadget>();

    let err = runner
        .assert_offline_schema_matches_migrations()
        .await
        .expect_err("a model with no migration must be reported as drift");

    use fundamental_core::Error as _;
    assert_eq!(err.error_code(), "MIGRATE_005");
}

#[derive(Clone, Debug, fundamental_macros::Model)]
#[model(sql, offline, syncable, table = "notes")]
struct Note {
    #[model(id)]
    id: uuid::Uuid,
    body: String,
    #[model(no_sync)]
    draft: String,
}

/// A syncable model's offline mirror carries the sync overlay and excludes `no_sync`.
/// The mirror is [`mirror_table_schema`], not `OfflineModel::create_table_sql`, which carries no
/// overlay.
#[test]
fn a_syncable_model_s_mirror_carries_the_overlay_and_not_the_no_sync_field() {
    use fundamental_sync::mirror_table_schema;

    let schema = mirror_table_schema::<Note>();
    let columns: Vec<&str> = schema.columns.keys().map(String::as_str).collect();

    assert!(
        columns.contains(&"body"),
        "synced column present: {columns:?}"
    );
    for overlay in ["updated_at", "version", "_tombstone"] {
        assert!(
            columns.contains(&overlay),
            "overlay `{overlay}` present: {columns:?}"
        );
    }
    assert!(
        !columns.contains(&"draft"),
        "a no_sync field must not reach the mirror: {columns:?}"
    );
}
