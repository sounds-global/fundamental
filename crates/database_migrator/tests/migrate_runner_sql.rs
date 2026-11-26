//! End-to-end test of [`MigrateRunner`] against a real node (Docker): apply a
//! hand-written migration, confirm re-applying is idempotent, and confirm `check`
//! reports nothing unapplied once it has run.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_db::migration::{Lints, MigrationConfig, MigrationGuard};
/// A real guard, so these runs exercise the refusal path a fleet ships with.
fn guard() -> MigrationGuard {
    MigrationGuard::new(1_000, Lints::default(), &[])
}

use fundamental_db_migrator::MigrateRunner;
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;
use tempfile::TempDir;

#[derive(Clone, Debug, Model)]
#[model(sql, table = "widgets")]
struct Widget {
    #[model(id)]
    id: uuid::Uuid,
    name: String,
    count: Option<i64>,
}

#[tokio::test]
async fn apply_is_idempotent_and_check_agrees_afterwards() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    let tmp = TempDir::new().expect("temp dir");
    let runner = MigrateRunner::new(
        MigrationConfig {
            migrations_dir: tmp.path().to_path_buf(),
            environment: "test".to_string(),
            is_protected: false,
        },
        guard(),
    )
    .sql_model::<Widget>();

    std::fs::create_dir_all(tmp.path().join("sql")).expect("migration dir");
    std::fs::write(
        tmp.path().join("sql/0001_create_widget.up.sql"),
        "CREATE TABLE widgets (id UUID NOT NULL, name TEXT NOT NULL, count BIGINT, \
         PRIMARY KEY (id HASH));",
    )
    .expect("write migration");

    let applied = runner.apply_sql(&pool).await.expect("apply");
    assert_eq!(applied.len(), 1);
    let again = runner.apply_sql(&pool).await.expect("re-apply");
    assert!(again.is_empty(), "already-applied migration is skipped");

    let check = runner.check_sql(&pool).await.expect("check");
    assert!(
        !check.has_unapplied(),
        "nothing should be unapplied: {}",
        check.summary()
    );
    assert!(
        check.summary().contains("cannot see"),
        "a clean check still says what it did not examine: {}",
        check.summary()
    );
}

#[tokio::test]
async fn check_refuses_an_unapplied_migration() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    let tmp = TempDir::new().expect("temp dir");
    let runner = MigrateRunner::new(
        MigrationConfig {
            migrations_dir: tmp.path().to_path_buf(),
            environment: "test".to_string(),
            is_protected: false,
        },
        guard(),
    )
    .sql_model::<Widget>();

    std::fs::create_dir_all(tmp.path().join("sql")).expect("migration dir");
    std::fs::write(
        tmp.path().join("sql/0001_create_widget.up.sql"),
        "CREATE TABLE widgets (id UUID NOT NULL, name TEXT NOT NULL, count BIGINT, \
         PRIMARY KEY (id HASH));",
    )
    .expect("write migration");

    let refused = runner.check_sql(&pool).await;
    let message = refused
        .expect_err("an unapplied migration is a refusal")
        .to_string();
    assert!(
        message.contains("0001_create_widget"),
        "the refusal names the migration: {message}"
    );
}

/// A column a later migration adds sits last in the table, wherever its field sits in the
/// model: the engine appends it and never reorders, and rows are read by name, so the order
/// is no drift.
#[derive(Clone, Debug, Model)]
#[model(sql, table = "gadgets")]
struct Gadget {
    #[model(id)]
    id: uuid::Uuid,
    label: Option<String>,
    count: i64,
}

#[tokio::test]
async fn a_column_added_later_is_no_drift_where_the_model_places_it() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    let tmp = TempDir::new().expect("temp dir");
    let runner = MigrateRunner::new(
        MigrationConfig {
            migrations_dir: tmp.path().to_path_buf(),
            environment: "test".to_string(),
            is_protected: false,
        },
        guard(),
    )
    .sql_model::<Gadget>();

    std::fs::create_dir_all(tmp.path().join("sql")).expect("migration dir");
    std::fs::write(
        tmp.path().join("sql/0001_create_gadget.up.sql"),
        "CREATE TABLE gadgets (id UUID NOT NULL, count BIGINT NOT NULL, PRIMARY KEY (id HASH));",
    )
    .expect("write migration");
    std::fs::write(
        tmp.path().join("sql/0002_gadget_label.up.sql"),
        "ALTER TABLE gadgets ADD COLUMN label TEXT;",
    )
    .expect("write migration");

    runner.apply_sql(&pool).await.expect("apply");
    let check = runner.check_sql(&pool).await.expect("check");
    assert!(check.drift.is_empty(), "{:#?}", check.drift);
}
