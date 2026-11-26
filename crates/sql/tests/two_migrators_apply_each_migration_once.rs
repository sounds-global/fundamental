//! Two processes applying the same migrations at once apply each migration exactly once.

use fundamental_db::migration::{Lints, MigrationGuard};
use fundamental_sql::runner::SqlMigrator;
use fundamental_sql::{SqlConfig, SqlPool};
use fundamental_test_containers::sql::SqlSchema;
use sqlx::Row as _;
use tracing_test::traced_test;

fn guard() -> MigrationGuard {
    MigrationGuard::new(1_000, Lints::default(), &[])
}

const MIGRATIONS: &[(&str, &str)] = &[
    // Long enough that the second migrator reads the ledger before the first commits.
    (
        "0001_once",
        "CREATE TABLE applied_once (id INT NOT NULL, PRIMARY KEY (id HASH)); SELECT pg_sleep(0.5);",
    ),
    ("0002_seed", "INSERT INTO applied_once (id) VALUES (1);"),
    (
        "0003_more",
        "CREATE TABLE applied_once_too (id INT NOT NULL, PRIMARY KEY (id HASH));",
    ),
];

async fn another_process(schema: &SqlSchema) -> SqlPool {
    let config: SqlConfig = schema.pool().config().clone();
    SqlPool::connect(&config)
        .await
        .expect("a second connection pool")
}

#[tokio::test]
#[traced_test]
async fn two_migrators_apply_each_migration_once() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let first = schema.pool().clone();
    let second = another_process(&schema).await;
    let (a, b) = (SqlMigrator::new(guard()), SqlMigrator::new(guard()));

    let (left, right) = tokio::join!(
        a.apply_all(&first, MIGRATIONS),
        b.apply_all(&second, MIGRATIONS)
    );
    let left = left.expect("the first migrator succeeds");
    let right = right.expect("the second migrator succeeds rather than tripping on the first");

    let mut applied: Vec<String> = left.into_iter().chain(right).collect();
    applied.sort();
    assert_eq!(
        applied,
        ["0001_once", "0002_seed", "0003_more"],
        "every migration is applied by exactly one of them"
    );
    let rows = first
        .fetch_all("SELECT count(*) AS seeded FROM applied_once")
        .await
        .expect("the seeded table");
    assert_eq!(rows[0].get::<i64, _>("seeded"), 1, "the seed ran once");
    let ledger = first
        .fetch_all(&format!(
            "SELECT count(*) AS recorded FROM {}",
            first.schema().qualify("_fundamental_migrations")
        ))
        .await
        .expect("the ledger");
    assert_eq!(ledger[0].get::<i64, _>("recorded"), 3);
    assert!(
        logs_contain("migration applied by another process while this one waited"),
        "the two contended, and the one that waited found the other's work"
    );
}
