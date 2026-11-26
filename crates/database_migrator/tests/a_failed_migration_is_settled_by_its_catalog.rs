//! `resolve --applied` records a failed migration as applied only once the live catalog shows
//! its effect, and names what is missing until it does.
//!
//! The migration builds an index after a step that times out at peak, as a deploy under load
//! meets it; the operator builds the index by hand off-peak and settles the migration.

#![cfg(not(target_arch = "wasm32"))]

use std::time::Duration;

use fundamental_core::Error as _;
use fundamental_db::migration::generate::SchemaItem;
use fundamental_db::migration::{
    LedgerError, LedgerState, Level, Lint, Lints, MigrationConfig, MigrationError, MigrationGuard,
    Resolution, Resolve,
};
use fundamental_db_migrator::MigrateRunner;
use fundamental_sql::{SqlConfig, SqlPool};
use fundamental_test_containers::sql::SqlSchema;
use tempfile::TempDir;

fn runner(dir: &TempDir) -> MigrateRunner {
    MigrateRunner::new(
        MigrationConfig {
            migrations_dir: dir.path().to_path_buf(),
            environment: "test".to_owned(),
            is_protected: false,
        },
        // Its subject is a backfill beside the column it fills, which the lints warn of.
        MigrationGuard::new(
            1_000,
            Lints::default()
                .with(Lint::Backfill, Level::Allow)
                .with(Lint::DdlBesideData, Level::Allow),
            &[],
        ),
    )
}

async fn pool(schema: &SqlSchema, timeout: Option<Duration>) -> SqlPool {
    let config = SqlConfig::development(schema.url())
        .default_schema(schema.schema().clone())
        .statement_timeout(timeout);
    SqlPool::connect(&config).await.expect("a pool")
}

fn write(dir: &TempDir, name: &str, sql: &str) {
    std::fs::create_dir_all(dir.path().join("sql")).expect("the directory");
    std::fs::write(dir.path().join(format!("sql/{name}.up.sql")), sql).expect("the file");
}

#[tokio::test]
async fn a_failed_migration_is_settled_by_its_catalog() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let at_peak = pool(&schema, Some(Duration::from_secs(2))).await;
    let off_peak = pool(&schema, None).await;
    let dir = TempDir::new().expect("a directory");
    let runner = runner(&dir);

    write(
        &dir,
        "0001_orders",
        "CREATE TABLE IF NOT EXISTS orders (id UUID NOT NULL, customer_id UUID, \
         PRIMARY KEY (id HASH));",
    );
    runner.apply_sql(&off_peak).await.expect("the table");
    write(
        &dir,
        "0002_index_orders",
        "SELECT pg_sleep(4);\n\
         CREATE INDEX IF NOT EXISTS orders_by_customer ON orders (customer_id HASH);",
    );
    runner
        .apply_sql(&at_peak)
        .await
        .expect_err("step 1 of 2 times out");

    let resolution = Resolution::new("built by hand off-peak").expect("a resolution");
    the_missing_index_is_named(&runner, &off_peak, &resolution).await;

    off_peak
        .execute_ddl("CREATE INDEX orders_by_customer ON orders (customer_id HASH)")
        .await
        .expect("the index, by hand");
    let settled = runner
        .resolve_sql(
            &off_peak,
            "0002_index_orders",
            Resolve::Applied,
            &resolution,
        )
        .await
        .expect("the catalog shows it now");
    assert_eq!((settled.step, settled.total), (1, 2));

    the_ledger_records_it(&runner, &off_peak).await;
}

/// Refused, naming the index the live catalog lacks.
async fn the_missing_index_is_named(
    runner: &MigrateRunner,
    off_peak: &SqlPool,
    resolution: &Resolution,
) {
    let refused = runner
        .resolve_sql(off_peak, "0002_index_orders", Resolve::Applied, resolution)
        .await
        .expect_err("the index is not there yet");
    let MigrationError::Ledger(LedgerError::NotInCatalog { missing, .. }) = &refused else {
        panic!("expected NotInCatalog, got {refused:?}");
    };
    assert!(
        missing.iter().any(|difference| difference.table == "orders"
            && difference.item == SchemaItem::Index("orders_by_customer".to_owned())),
        "{missing:?}"
    );
    assert_eq!(refused.error_code(), "MIGRATE_019");
    assert!(
        refused
            .to_string()
            .contains("index orders_by_customer on orders is missing"),
        "{refused}"
    );
}

/// The ledger records it applied and why, and nothing is left to apply.
async fn the_ledger_records_it(runner: &MigrateRunner, off_peak: &SqlPool) {
    let ledger = runner.status_sql(off_peak).await.expect("the ledger");
    let LedgerState::Applied {
        resolved: Some(resolved),
        ..
    } = ledger.entries()[1].state()
    else {
        panic!("expected it applied and resolved: {ledger:?}");
    };
    assert_eq!(resolved.act, Resolve::Applied);
    assert_eq!(resolved.reason, "built by hand off-peak");
    assert!(ledger.is_applied(), "{ledger}");
    assert!(
        runner.apply_sql(off_peak).await.expect("apply").is_empty(),
        "nothing is left to apply"
    );
}

/// A backfill after an `ADD COLUMN` leaves the column in the catalog whether or not its rows
/// were written, so `--applied` is refused, naming the step, and `--retry` settles it.
#[tokio::test]
async fn a_backfill_the_catalog_cannot_show_is_not_marked_applied() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let at_peak = pool(&schema, Some(Duration::from_secs(2))).await;
    let off_peak = pool(&schema, None).await;
    let dir = TempDir::new().expect("a directory");
    let runner = runner(&dir);

    write(
        &dir,
        "0001_orders",
        "CREATE TABLE IF NOT EXISTS orders (id UUID NOT NULL, PRIMARY KEY (id HASH));\n\
         INSERT INTO orders (id) VALUES ('7f9c2a4e-0d1b-4c3a-9e8f-1a2b3c4d5e6f') \
         ON CONFLICT DO NOTHING;",
    );
    runner.apply_sql(&off_peak).await.expect("the table");
    write(
        &dir,
        "0002_orders_note",
        "ALTER TABLE orders ADD COLUMN IF NOT EXISTS note TEXT;\n\
         UPDATE orders SET note = (SELECT 'backfilled' FROM pg_sleep(4));",
    );
    runner
        .apply_sql(&at_peak)
        .await
        .expect_err("step 2 of 2 times out");

    let resolution = Resolution::new("backfill timed out at peak").expect("a resolution");
    let refused = runner
        .resolve_sql(&off_peak, "0002_orders_note", Resolve::Applied, &resolution)
        .await
        .expect_err("the column is there, the backfill is not shown");
    let MigrationError::Ledger(LedgerError::StepNotInCatalog { step, total, .. }) = &refused else {
        panic!("expected StepNotInCatalog, got {refused:?}");
    };
    assert_eq!((*step, *total), (2, 2));
    assert_eq!(refused.error_code(), "MIGRATE_023");

    let settled = runner
        .resolve_sql(&off_peak, "0002_orders_note", Resolve::Retry, &resolution)
        .await
        .expect("the backfill runs off-peak");
    assert_eq!(settled.step, 2);
    assert!(
        runner
            .status_sql(&off_peak)
            .await
            .expect("the ledger")
            .is_applied()
    );
}
