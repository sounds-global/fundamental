//! `verify_sql` replays a set's files from scratch and as the upgrade from its base commit,
//! and names what the catalog holds otherwise than the committed snapshot says.
//!
//! Requires Docker. Each test changes the working directory, so needs a process of its own.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::process::Command;

use fundamental_core::Error as _;
use fundamental_core::model::sql::ShardKind;
use fundamental_db::migration::generate::{SchemaItem, Snapshot};
use fundamental_db::migration::{
    EmbeddedMigration, EmbeddedMigrations, KeyPart, Lints, MigrationConfig, MigrationError,
    MigrationGuard, TableColumn, TableSchema, content_hash,
};
use fundamental_db_migrator::MigrateRunner;
use fundamental_test_containers::sql::SqlSchema;
use tempfile::TempDir;

const FIRST: &str = "CREATE TABLE IF NOT EXISTS verify_widgets (id UUID NOT NULL, \
                     PRIMARY KEY (id HASH));\n";
const SECOND: &str = "ALTER TABLE verify_widgets ADD COLUMN IF NOT EXISTS label TEXT;\n";

fn widgets() -> TableSchema {
    let mut table = TableSchema::new("verify_widgets");
    for (name, nullable) in [("id", false), ("label", true)] {
        table.add_column(TableColumn {
            name: name.to_owned(),
            data_type: if name == "id" { "UUID" } else { "TEXT" }.to_owned(),
            nullable,
            default: None,
            primary_key: false,
            unique: false,
        });
    }
    table.set_key(KeyPart::parts(&[("id", Some(ShardKind::Hash))]));
    table
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

/// A repository whose base commit holds the set's first file, with the second written on the
/// branch, and the set as a crate built on the branch embeds it, with `snapshot` committed.
fn branch(snapshot: &Snapshot) -> (TempDir, &'static EmbeddedMigrations) {
    let repository = TempDir::new().expect("temp dir");
    let sql = repository.path().join("acme/migrations/sql");
    std::fs::create_dir_all(&sql).expect("dirs");
    std::fs::write(sql.join("0001_widgets.up.sql"), FIRST).expect("first file");
    git(repository.path(), &["init", "-q"]);
    git(repository.path(), &["add", "."]);
    git(repository.path(), &["commit", "-q", "-m", "base"]);
    git(repository.path(), &["branch", "base"]);
    std::fs::write(sql.join("0002_widgets_label.up.sql"), SECOND).expect("second file");

    let dir: &'static str = repository
        .path()
        .join("acme")
        .to_string_lossy()
        .into_owned()
        .leak();
    let files: &'static [EmbeddedMigration] = Vec::leak(vec![
        EmbeddedMigration::new("0001_widgets", FIRST),
        EmbeddedMigration::new("0002_widgets_label", SECOND),
    ]);
    let tables: &'static [fn() -> TableSchema] = Vec::leak(vec![widgets as fn() -> TableSchema]);
    let set: &'static EmbeddedMigrations = Box::leak(Box::new(
        EmbeddedMigrations::sql("acme", files, tables).generated(snapshot.to_file().leak(), dir),
    ));
    std::env::set_current_dir(repository.path()).expect("into the repository");
    (repository, set)
}

fn runner(set: &'static EmbeddedMigrations, app: &Path) -> MigrateRunner {
    MigrateRunner::new(
        MigrationConfig {
            migrations_dir: app.to_path_buf(),
            environment: "test".to_owned(),
            is_protected: false,
        },
        MigrationGuard::new(1_000, Lints::default(), &[]),
    )
    .with_embedded(set)
}

#[tokio::test]
async fn a_branch_whose_files_make_its_snapshot_is_verified() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let snapshot = Snapshot::of(&[widgets()], Some(content_hash(SECOND)));
    let (repository, set) = branch(&snapshot);

    let verification = runner(set, &repository.path().join("apps/main/migrations"))
        .verify_sql(schema.pool(), "base")
        .await
        .expect("the files make the snapshot");

    assert_eq!(
        (
            verification.from_scratch,
            verification.at_base,
            verification.branch
        ),
        (2, 1, 1)
    );
}

#[tokio::test]
async fn a_snapshot_the_files_do_not_make_is_named_exactly() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let mut said = widgets();
    if let Some(label) = said.columns.get_mut("label") {
        label.nullable = false;
    }
    let snapshot = Snapshot::of(&[said], Some(content_hash(SECOND)));
    let (repository, set) = branch(&snapshot);

    let error = runner(set, &repository.path().join("apps/main/migrations"))
        .verify_sql(schema.pool(), "base")
        .await
        .expect_err("the snapshot says NOT NULL");

    assert_eq!(error.error_code(), "MIGRATE_011");
    let MigrationError::Unverified { differences, .. } = &error else {
        panic!("{error:?}");
    };
    assert!(
        differences.iter().all(|difference| difference.item
            == SchemaItem::Column("label".to_owned())
            && difference.expected.as_deref() == Some("TEXT NOT NULL")
            && difference.found.as_deref() == Some("TEXT NULL")),
        "{error}"
    );
    assert_eq!(
        differences.len(),
        2,
        "from scratch and the upgrade: {error}"
    );
}
