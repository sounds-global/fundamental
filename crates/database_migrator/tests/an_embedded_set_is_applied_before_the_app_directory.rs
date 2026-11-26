//! A crate's embedded migrations run in the app's ledger, before the app's own, under
//! `<owner>/<name>`, hash-checked, and with their models in the drift check.
//!
//! Requires Docker: the ledger and the tables are a live database's.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_core::Error as _;
use fundamental_core::model::SqlDb;
use fundamental_db::migration::{
    EmbeddedMigration, EmbeddedMigrations, Lints, MigrationConfig, MigrationError, MigrationGuard,
    ModelSchema, TableSchema,
};
use fundamental_db_migrator::MigrateRunner;
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;
use tempfile::TempDir;

#[derive(Clone, Debug, Model)]
#[model(sql, table = "gadgets")]
struct Gadget {
    #[model(id)]
    id: uuid::Uuid,
    label: String,
}

static GADGET_TABLES: [fn() -> TableSchema; 1] = [<Gadget as ModelSchema<SqlDb>>::table_schema];

static GADGETS: [EmbeddedMigration; 1] = [EmbeddedMigration::new(
    "0001_gadgets",
    "CREATE TABLE IF NOT EXISTS gadgets (id UUID NOT NULL, label TEXT NOT NULL, \
     PRIMARY KEY (id HASH));",
)];

static GADGET_SET: EmbeddedMigrations =
    EmbeddedMigrations::sql("fundamental_gadgets", &GADGETS, &GADGET_TABLES);

static GADGETS_EDITED: [EmbeddedMigration; 1] = [EmbeddedMigration::new(
    "0001_gadgets",
    "CREATE TABLE IF NOT EXISTS gadgets (id UUID NOT NULL, label TEXT, \
     PRIMARY KEY (id HASH));",
)];

static GADGET_SET_EDITED: EmbeddedMigrations =
    EmbeddedMigrations::sql("fundamental_gadgets", &GADGETS_EDITED, &GADGET_TABLES);

static WIDGETS: [EmbeddedMigration; 1] = [EmbeddedMigration::new(
    "0001_widgets",
    "CREATE TABLE IF NOT EXISTS widgets (id UUID NOT NULL, PRIMARY KEY (id HASH));",
)];

static WIDGET_SET: EmbeddedMigrations =
    EmbeddedMigrations::sql("fundamental_widgets", &WIDGETS, &[]);

static UNSAFE: [EmbeddedMigration; 1] = [EmbeddedMigration::new("0001_x", "SELECT 1;")];

static UNSAFE_SET: EmbeddedMigrations = EmbeddedMigrations::sql("../escape", &UNSAFE, &[]);

fn runner(dir: &TempDir) -> MigrateRunner {
    MigrateRunner::new(
        MigrationConfig {
            migrations_dir: dir.path().to_path_buf(),
            environment: "test".to_string(),
            is_protected: false,
        },
        MigrationGuard::new(1_000, Lints::default(), &[]),
    )
}

/// An app directory whose one migration references the embedded table, so it can only
/// apply if the embedded set ran first.
fn app_dir() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("sql")).expect("migration dir");
    std::fs::write(
        dir.path().join("sql/0001_gadget_notes.up.sql"),
        "CREATE TABLE IF NOT EXISTS gadget_notes (gadget_id UUID NOT NULL \
         REFERENCES gadgets (id), note TEXT NOT NULL, PRIMARY KEY (gadget_id HASH));",
    )
    .expect("write migration");
    dir
}

#[tokio::test]
async fn an_embedded_set_is_applied_before_the_app_directory_and_tracked_under_its_owner() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    let dir = app_dir();
    let runner = runner(&dir).with_embedded(&GADGET_SET);

    let unapplied = runner.unapplied_sql(&pool).await.expect("unapplied");
    assert_eq!(
        unapplied,
        ["fundamental_gadgets/0001_gadgets", "0001_gadget_notes"],
        "the embedded set is listed first, under its owner"
    );

    let applied = runner.apply_sql(&pool).await.expect("apply");
    assert_eq!(
        applied,
        ["fundamental_gadgets/0001_gadgets", "0001_gadget_notes"]
    );
    assert!(
        runner.apply_sql(&pool).await.expect("re-apply").is_empty(),
        "an applied embedded migration is skipped, not run twice"
    );

    let check = runner.check_sql(&pool).await.expect("check");
    assert!(!check.has_unapplied(), "{}", check.summary());
    assert!(
        !check
            .drift
            .iter()
            .any(|drift| format!("{drift:?}").contains("gadgets")),
        "the embedded model matches the embedded DDL: {:?}",
        check.drift
    );
}

#[tokio::test]
async fn an_edited_embedded_migration_is_refused_by_hash() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    let dir = TempDir::new().expect("temp dir");

    runner(&dir)
        .with_embedded(&GADGET_SET)
        .apply_sql(&pool)
        .await
        .expect("the original applies");

    let error = runner(&dir)
        .with_embedded(&GADGET_SET_EDITED)
        .apply_sql(&pool)
        .await
        .expect_err("an edited applied migration is refused");
    assert_eq!(error.error_code(), "SQL_013", "got {error:?}");
}

#[tokio::test]
async fn an_owner_that_could_be_a_path_is_refused() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let dir = TempDir::new().expect("temp dir");

    let error = runner(&dir)
        .with_embedded(&UNSAFE_SET)
        .apply_sql(schema.pool())
        .await
        .expect_err("an owner holding a separator never reaches the ledger");
    assert!(
        matches!(error, MigrationError::UnsafeName { .. }),
        "got {error:?}"
    );
}

#[tokio::test]
async fn a_staged_apply_stops_after_the_set_it_names() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    let dir = app_dir();
    let runner = runner(&dir)
        .with_embedded(&GADGET_SET)
        .with_embedded(&WIDGET_SET);

    let staged = runner
        .apply_sql_through(&pool, &GADGET_SET)
        .await
        .expect("apply through the first set");
    assert_eq!(staged, ["fundamental_gadgets/0001_gadgets"]);
    assert_eq!(
        runner.unapplied_sql(&pool).await.expect("unapplied"),
        ["fundamental_widgets/0001_widgets", "0001_gadget_notes"],
        "neither a later set nor the app's directory is applied"
    );

    let rest = runner.apply_sql(&pool).await.expect("apply the rest");
    assert_eq!(
        rest,
        ["fundamental_widgets/0001_widgets", "0001_gadget_notes"]
    );
}

#[tokio::test]
async fn a_staged_apply_through_an_unregistered_set_is_refused() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    let dir = app_dir();
    let runner = runner(&dir).with_embedded(&GADGET_SET);

    let error = runner
        .apply_sql_through(&pool, &WIDGET_SET)
        .await
        .expect_err("a set the runner does not hold is refused");
    assert!(
        matches!(&error, MigrationError::SetNotRegistered { owner } if owner == "fundamental_widgets"),
        "got {error:?}"
    );
    assert_eq!(error.error_code(), "MIGRATE_010");
    assert!(
        runner.applied_sql(&pool).await.expect("applied").is_empty(),
        "nothing is applied when the set is refused"
    );
}

#[tokio::test]
async fn a_staged_apply_refused_later_on_applies_nothing() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    let dir = TempDir::new().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("sql")).expect("migration dir");
    std::fs::write(
        dir.path().join("sql/0001_drop_gadgets.up.sql"),
        "DROP TABLE gadgets;",
    )
    .expect("write migration");
    let runner = runner(&dir).with_embedded(&GADGET_SET);

    runner
        .apply_sql_through(&pool, &GADGET_SET)
        .await
        .expect_err("the guard refuses the app's destructive migration before the set applies");
    assert!(
        runner.applied_sql(&pool).await.expect("applied").is_empty(),
        "nothing before the refused migration is applied"
    );
}
