//! A model with `Localized` text expects its companion table on its own engine, so the drift
//! check knows the table and a migration that creates it replays into the expected schema.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;

use fundamental_core::i18n::Localized;
use fundamental_db::migration::{
    MigrationConfig, MigrationDatabase, MigrationError, MigrationGuard,
};
use fundamental_db_migrator::MigrateRunner;
use fundamental_macros::Model;
use tempfile::TempDir;

/// A notice of one organisation, newest first, with a title in several languages.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(cql, table = "notices")]
struct Notice {
    #[model(partition_key)]
    organisation_id: uuid::Uuid,
    #[model(clustering_key = "desc")]
    id: i64,
    title: Localized<String>,
}

/// A draft keyed by an integer, with a title in several languages.
#[derive(Clone, Debug, Model)]
#[model(offline)]
struct Draft {
    #[model(id)]
    id: i64,
    title: Localized<String>,
}

/// A tag with no text in other languages.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(cql, table = "tags")]
struct Tag {
    #[model(partition_key)]
    name: String,
}

fn config(dir: &Path) -> MigrationConfig {
    MigrationConfig {
        migrations_dir: dir.to_path_buf(),
        environment: "test".to_string(),
        is_protected: false,
    }
}

#[test]
fn a_cql_models_companion_is_in_the_expected_schema() {
    let tmp = TempDir::new().expect("temp dir");
    let runner =
        MigrateRunner::new(config(tmp.path()), MigrationGuard::unenforced()).cql_model::<Notice>();

    let expected = runner.expected_schema(MigrationDatabase::CqlDb);

    let companion = expected
        .tables
        .get("notices_i18n")
        .expect("the companion is expected");
    assert_eq!(
        companion.partition_key.as_deref(),
        Some(&["organisation_id".to_owned()][..])
    );
}

#[test]
fn a_cql_model_without_localized_text_expects_no_companion() {
    let tmp = TempDir::new().expect("temp dir");
    let runner =
        MigrateRunner::new(config(tmp.path()), MigrationGuard::unenforced()).cql_model::<Tag>();

    let expected = runner.expected_schema(MigrationDatabase::CqlDb);

    assert_eq!(
        expected.tables.keys().collect::<Vec<_>>(),
        ["tags"],
        "only the model's own table"
    );
}

#[tokio::test]
async fn an_offline_migration_creating_the_companion_replays_into_the_expected_schema() {
    let tmp = TempDir::new().expect("temp dir");
    let offline = tmp.path().join("offline");
    std::fs::create_dir_all(&offline).expect("migration dir");
    std::fs::write(
        offline.join("0001_create_draft.up.sql"),
        "CREATE TABLE IF NOT EXISTS draft (id INTEGER NOT NULL, title TEXT NOT NULL, \
         PRIMARY KEY (id));\n\
         CREATE TABLE IF NOT EXISTS draft_i18n (id INTEGER NOT NULL, field TEXT NOT NULL, \
         locale TEXT NOT NULL, value TEXT NOT NULL, source_hash TEXT NOT NULL DEFAULT '', \
         translation_hash TEXT NOT NULL DEFAULT '', checked_by TEXT, checked_at INTEGER, \
         PRIMARY KEY (id, field, locale));",
    )
    .expect("write migration");
    let runner = MigrateRunner::new(config(tmp.path()), MigrationGuard::unenforced())
        .offline_model::<Draft>();

    assert!(
        runner
            .expected_schema(MigrationDatabase::OfflineDb)
            .tables
            .contains_key("draft_i18n")
    );
    runner
        .assert_offline_schema_matches_migrations()
        .await
        .expect("the migration replays into the models' schema, companion and all");
}

#[tokio::test]
async fn an_offline_migration_without_the_companion_is_drift() {
    let tmp = TempDir::new().expect("temp dir");
    let offline = tmp.path().join("offline");
    std::fs::create_dir_all(&offline).expect("migration dir");
    std::fs::write(
        offline.join("0001_create_draft.up.sql"),
        "CREATE TABLE IF NOT EXISTS draft (id INTEGER NOT NULL, title TEXT NOT NULL, \
         PRIMARY KEY (id));",
    )
    .expect("write migration");
    let runner = MigrateRunner::new(config(tmp.path()), MigrationGuard::unenforced())
        .offline_model::<Draft>();

    let refused = runner.assert_offline_schema_matches_migrations().await;

    let Err(MigrationError::SchemaVerification { drifts, .. }) = refused else {
        panic!("a missing companion is schema drift, not {refused:?}");
    };
    let tables: Vec<&str> = drifts
        .iter()
        .map(|drift| drift.table_name.as_str())
        .collect();
    assert_eq!(
        tables,
        ["draft_i18n"],
        "the companion, and only it, is missing"
    );
}
