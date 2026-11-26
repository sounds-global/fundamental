//! Two processes applying the same YCQL migrations at once apply each statement exactly once.
//!
//! YCQL has no transaction to hold a lock in and no rollback, so a migration run twice runs its
//! statements twice: a non-idempotent one — an `ALTER TABLE … ADD`, a seed `INSERT` of a
//! counter — then fails one migrator or corrupts the keyspace. Each migrator here holds its own
//! session, as a separate process would.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_cql::runner::CqlMigrator;
use fundamental_cql::{CqlIdent, CqlPool, CqlValue, cql, ddl};
use fundamental_db::migration::{Lints, MigrationGuard};
use fundamental_test_containers::cql::{CqlKeyspace, CqlNode};
use tracing_test::traced_test;

fn guard() -> MigrationGuard {
    MigrationGuard::new(1_000, Lints::default(), &[])
}

/// Neither `ALTER TABLE … ADD` is idempotent: the column exists the second time.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_table",
        "CREATE TABLE applied_once (id text PRIMARY KEY, seen int);",
    ),
    ("0002_column", "ALTER TABLE applied_once ADD note text;"),
    ("0003_column", "ALTER TABLE applied_once ADD other text;"),
];

#[tokio::test]
#[traced_test]
async fn two_migrators_apply_each_migration_once() {
    let node = CqlNode::shared().await.expect("a YCQL node");
    let keyspace = CqlKeyspace::mint(node).await.expect("a test keyspace");
    let first = keyspace.pool().clone();
    let second = CqlPool::connect(&node.config(Some(keyspace.name())))
        .await
        .expect("a second session, as a second process holds");
    let (a, b) = (CqlMigrator::new(guard()), CqlMigrator::new(guard()));

    let (left, right) = tokio::join!(
        a.apply_all(&first, MIGRATIONS),
        b.apply_all(&second, MIGRATIONS)
    );
    let left = left.expect("the first migrator succeeds");
    let right = right.expect("the second migrator succeeds rather than re-running the first's");

    let mut applied: Vec<String> = left.into_iter().chain(right).collect();
    applied.sort();
    assert_eq!(applied, ["0001_table", "0002_column", "0003_column"]);
    let recorded: Vec<(String,)> = first
        .fetch_all(&cql!("SELECT name FROM _fundamental_migrations"))
        .await
        .expect("the ledger");
    assert_eq!(recorded.len(), 3);
    assert!(
        logs_contain("waiting for the migration lock"),
        "the two contended, and one waited for the other"
    );
}

/// A migrator that died holding the lock stops blocking the next one when its lease's TTL
/// runs out: the next waits for that, then applies — it neither fails nor runs beside it.
#[tokio::test]
#[traced_test]
async fn a_lease_left_by_a_dead_migrator_expires_and_the_next_one_applies() {
    let node = CqlNode::shared().await.expect("a YCQL node");
    let keyspace = CqlKeyspace::mint(node).await.expect("a test keyspace");
    let pool = keyspace.pool().clone();
    let lock =
        CqlIdent::qualified(keyspace.name(), "_fundamental_migration_lock").expect("an identifier");
    pool.execute_ddl(&ddl!(
        "CREATE TABLE IF NOT EXISTS {} (name text PRIMARY KEY, holder text)",
        lock
    ))
    .await
    .expect("the lock table");
    pool.execute(
        &cql!(
            "INSERT INTO {} (name, holder) VALUES (?, ?) USING TTL ?",
            lock
        )
        .bind(CqlValue::Text("ledger".to_owned()))
        .bind(CqlValue::Text("a-migrator-that-died".to_owned()))
        .bind(CqlValue::Int(3)),
    )
    .await
    .expect("a lease nobody will release");

    let started = std::time::Instant::now();
    let applied = CqlMigrator::new(guard())
        .apply_all(&pool, &MIGRATIONS[..1])
        .await
        .expect("the lease expires and the migration applies");

    assert_eq!(applied, ["0001_table"]);
    assert!(
        started.elapsed() >= std::time::Duration::from_secs(2),
        "it waited for the dead migrator's lease rather than ignoring it"
    );
    assert!(logs_contain("waiting for the migration lock"));
}
