//! `resolve --applied` on YCQL records a failed migration as applied only once the live
//! keyspace shows its effect, replayed in a scratch keyspace beside it.
//!
//! A hand-made table with another key type fails step 2 in the live keyspace only.

#![cfg(not(target_arch = "wasm32"))]
#![recursion_limit = "256"]

use fundamental_core::Error as _;
use fundamental_cql::{CqlIdent, CqlPool, ddl};
use fundamental_db::migration::{
    LedgerError, LedgerState, Lints, MigrationConfig, MigrationError, MigrationGuard, Resolution,
    Resolve,
};
use fundamental_db_migrator::MigrateRunner;
use fundamental_test_containers::cql::{CqlKeyspace, CqlNode};
use tempfile::TempDir;

const ORDERS: &str = "CREATE TABLE IF NOT EXISTS orders (id text PRIMARY KEY);\n\
     CREATE TABLE invoices (id text PRIMARY KEY);";

fn runner(dir: &TempDir) -> MigrateRunner {
    MigrateRunner::new(
        MigrationConfig {
            migrations_dir: dir.path().to_path_buf(),
            environment: "test".to_owned(),
            is_protected: false,
        },
        MigrationGuard::new(1_000, Lints::default(), &[]),
    )
}

/// `CREATE TABLE invoices (id <key> PRIMARY KEY)` in the live keyspace, by hand.
async fn invoices_by_hand(pool: &CqlPool, keyspace: &str, key: &str) {
    let invoices = CqlIdent::qualified(keyspace, "invoices").expect("an identifier");
    let statement = match key {
        "int" => ddl!("CREATE TABLE {} (id int PRIMARY KEY)", invoices),
        _ => ddl!("CREATE TABLE {} (id text PRIMARY KEY)", invoices),
    };
    pool.execute_ddl(&statement)
        .await
        .expect("the table, by hand");
}

#[tokio::test]
async fn a_failed_cql_migration_is_settled_by_its_catalog() {
    let node = CqlNode::shared().await.expect("a YCQL node");
    let keyspace = CqlKeyspace::mint(node).await.expect("a test keyspace");
    let pool = keyspace.pool().clone();
    let dir = TempDir::new().expect("a directory");
    std::fs::create_dir_all(dir.path().join("cql")).expect("the directory");
    std::fs::write(dir.path().join("cql/0001_orders.up.cql"), ORDERS).expect("the file");
    let runner = runner(&dir);

    invoices_by_hand(&pool, keyspace.name(), "int").await;
    runner
        .apply_cql(&pool)
        .await
        .expect_err("step 2 meets the table made by hand");

    let resolution = Resolution::new("invoices was made by hand").expect("a resolution");
    let refused = runner
        .resolve_cql(&pool, "0001_orders", Resolve::Applied, &resolution)
        .await
        .expect_err("the live table is not the one the migration makes");
    assert!(
        matches!(
            &refused,
            MigrationError::Ledger(LedgerError::NotInCatalog { missing, .. })
                if missing.iter().all(|difference| difference.table == "invoices")
        ),
        "{refused:?}"
    );
    assert_eq!(refused.error_code(), "MIGRATE_019");

    let invoices = CqlIdent::qualified(keyspace.name(), "invoices").expect("an identifier");
    pool.execute_ddl(&ddl!("DROP TABLE {}", invoices))
        .await
        .expect("the wrong table, dropped");
    invoices_by_hand(&pool, keyspace.name(), "text").await;
    let settled = runner
        .resolve_cql(&pool, "0001_orders", Resolve::Applied, &resolution)
        .await
        .expect("the keyspace shows it now");
    assert_eq!((settled.step, settled.total), (2, 2));

    let ledger = runner.status_cql(&pool).await.expect("the ledger");
    assert!(
        matches!(
            ledger.entries()[0].state(),
            LedgerState::Applied { resolved: Some(resolved), .. } if resolved.act == Resolve::Applied
        ),
        "{ledger:?}"
    );
}
