//! An applied migration cannot change without the next run saying so.

use fundamental_db::migration::{Lints, MigrationGuard};
use fundamental_sql::SqlError;
use fundamental_sql::runner::SqlMigrator;
use fundamental_test_containers::sql::SqlSchema;

/// A real guard, so the probe SQL stays a shape a fleet would accept.
fn guard() -> MigrationGuard {
    MigrationGuard::new(1_000, Lints::default(), &[])
}

const CREATE: &str = "CREATE TABLE ledger_probe (id TEXT NOT NULL, PRIMARY KEY (id HASH));";
const REKEYED: &str = "CREATE TABLE ledger_probe (zone TEXT NOT NULL, id TEXT NOT NULL, \
     PRIMARY KEY ((zone, id) HASH));";

#[tokio::test]
async fn an_edited_migration_is_refused_rather_than_skipped() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let migrator = SqlMigrator::new(guard());

    migrator
        .apply_all(schema.pool(), &[("0001_probe", CREATE)])
        .await
        .expect("the first apply succeeds");

    let error = migrator
        .apply_all(schema.pool(), &[("0001_probe", REKEYED)])
        .await
        .expect_err("an edited migration must not be skipped as already applied");

    match error {
        SqlError::MigrationChanged {
            name,
            applied,
            found,
        } => {
            assert_eq!(name, "0001_probe");
            assert_ne!(applied, found, "the error must name two different hashes");
        }
        other => panic!("expected MigrationChanged, got {other:?}"),
    }
}

#[tokio::test]
async fn an_unchanged_migration_still_skips() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let migrator = SqlMigrator::new(guard());

    let first = migrator
        .apply_all(schema.pool(), &[("0001_probe", CREATE)])
        .await
        .expect("the first apply succeeds");
    assert_eq!(first.len(), 1);

    let second = migrator
        .apply_all(schema.pool(), &[("0001_probe", CREATE)])
        .await
        .expect("re-running an unchanged migration is not an error");
    assert!(second.is_empty(), "an unchanged migration applies once");
}

#[tokio::test]
async fn a_reworded_comment_is_not_a_change() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let migrator = SqlMigrator::new(guard());

    migrator
        .apply_all(schema.pool(), &[("0001_probe", CREATE)])
        .await
        .expect("the first apply succeeds");

    let recommented = format!("-- a sentence about why this table exists\n{CREATE}");
    migrator
        .apply_all(schema.pool(), &[("0001_probe", &recommented)])
        .await
        .expect("a comment is not the migration");
}

/// The runner reads `42P01` as nothing applied, so the engine must answer it and it must stay out of the retryable set.
#[tokio::test]
async fn a_missing_relation_is_undefined_table_and_stays_out_of_the_retryable_set() {
    let schema = SqlSchema::mint().await.expect("a test schema");

    let error = schema
        .pool()
        .fetch_all("SELECT 1 FROM a_relation_that_was_never_created LIMIT 0")
        .await
        .map(|rows| rows.len())
        .expect_err("reading a relation that does not exist must fail");

    assert!(
        error.is_undefined_table(),
        "the engine's undefined_table must be recognised as one, or the migration runner \
         waits on nothing: {error}"
    );
    assert!(
        !error.is_retryable(),
        "undefined_table must stay out of the retryable set: in it, a typo'd table name \
         becomes a bounded hang with a misleading log, everywhere in the framework: {error}"
    );
}
