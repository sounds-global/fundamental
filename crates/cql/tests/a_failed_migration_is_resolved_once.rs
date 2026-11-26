//! A YCQL migration that fails says where, blocks every later apply, and is settled by one
//! recorded act.
//!
//! Step 2 of 3 writes to a table nobody made, which the node refuses: an answer, so the step
//! did not apply and may run again once the table is there.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_core::Error as _;
use fundamental_cql::runner::CqlMigrator;
use fundamental_cql::{CqlError, CqlIdent, CqlPool, ddl};
use fundamental_db::migration::{
    LedgerError, LedgerState, Lints, MigrationGuard, Resolution, Resolve,
};
use fundamental_test_containers::cql::{CqlKeyspace, CqlNode};

fn guard() -> MigrationGuard {
    MigrationGuard::new(1_000, Lints::default(), &[])
}

const ORDERS: &str = "CREATE TABLE IF NOT EXISTS orders (id text PRIMARY KEY, note text);\n\
     INSERT INTO kinds (name) VALUES ('news');\n\
     CREATE TABLE IF NOT EXISTS invoices (id text PRIMARY KEY);";
const NOTES: &str = "CREATE TABLE IF NOT EXISTS notes (id text PRIMARY KEY);";

fn files() -> [(&'static str, &'static str); 2] {
    [("0001_orders", ORDERS), ("0002_notes", NOTES)]
}

#[tokio::test]
async fn a_failed_cql_migration_is_resolved_once() {
    let node = CqlNode::shared().await.expect("a YCQL node");
    let keyspace = CqlKeyspace::mint(node).await.expect("a test keyspace");
    let pool = keyspace.pool().clone();
    let migrator = CqlMigrator::new(guard());

    let error = migrator
        .apply_all(&pool, &files())
        .await
        .expect_err("step 2 writes to a missing table");
    assert!(
        matches!(
            &error,
            CqlError::MigrationFailed { name, step: 2, total: 3, statement, .. }
                if name == "0001_orders" && statement.starts_with("INSERT INTO kinds")
        ),
        "{error:?}"
    );

    status_says_where(&migrator, &pool).await;

    let refused = migrator
        .apply_all(&pool, &files())
        .await
        .expect_err("an unresolved migration blocks the apply");
    assert!(
        matches!(
            &refused,
            CqlError::Ledger(LedgerError::Unresolved { step: 2, .. })
        ),
        "{refused:?}"
    );
    assert_eq!(refused.error_code(), "MIGRATE_016");

    // The operator makes the table the step needs, and retries.
    let kinds = CqlIdent::qualified(keyspace.name(), "kinds").expect("an identifier");
    pool.execute_ddl(&ddl!("CREATE TABLE {} (name text PRIMARY KEY)", kinds))
        .await
        .expect("the missing table");
    let resolution = Resolution::new("kinds was never created; made by hand").expect("why");
    let settled = migrator
        .retry(&pool, ("0001_orders", ORDERS), &resolution)
        .await
        .expect("the retry finishes it");
    assert_eq!(settled.step, 2);

    let ledger = migrator.ledger(&pool, &files()).await.expect("the ledger");
    let LedgerState::Applied {
        resolved: Some(resolved),
        ..
    } = ledger.entries()[0].state()
    else {
        panic!("expected the first applied and resolved: {ledger:?}");
    };
    assert_eq!(resolved.act, Resolve::Retry);
    assert_eq!(resolved.by, resolution.by());
    assert_eq!(resolved.reason, "kinds was never created; made by hand");

    let applied = migrator
        .apply_all(&pool, &files())
        .await
        .expect("nothing is in the way");
    assert_eq!(applied, ["0002_notes"]);
}

/// Status says where the first migration stopped, and that the second waits behind it.
async fn status_says_where(migrator: &CqlMigrator, pool: &CqlPool) {
    let ledger = migrator.ledger(pool, &files()).await.expect("the ledger");
    let LedgerState::Failed(failed) = ledger.entries()[0].state() else {
        panic!("expected the first failed: {ledger:?}");
    };
    assert_eq!((failed.step, failed.total), (2, 3));
    assert!(!failed.answer_lost, "the node refused it: {failed:?}");
    assert!(
        matches!(ledger.entries()[1].state(), LedgerState::Blocked { .. }),
        "{ledger:?}"
    );
}

#[tokio::test]
async fn status_of_a_keyspace_never_migrated_makes_nothing() {
    let node = CqlNode::shared().await.expect("a YCQL node");
    let keyspace = CqlKeyspace::mint(node).await.expect("a test keyspace");
    let pool = keyspace.pool().clone();
    let ledger = CqlMigrator::new(guard())
        .ledger(&pool, &files())
        .await
        .expect("the ledger reads");
    assert!(
        ledger
            .entries()
            .iter()
            .all(|entry| matches!(entry.state(), LedgerState::Pending)),
        "{ledger}"
    );
    let tables: Vec<(String,)> = pool
        .fetch_all(
            &fundamental_cql::Cql::new(
                "SELECT table_name FROM system_schema.tables WHERE keyspace_name = ?",
            )
            .bind(fundamental_cql::CqlValue::text(keyspace.name())),
        )
        .await
        .expect("the tables");
    assert!(tables.is_empty(), "{tables:?}");
}
