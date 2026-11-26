//! A transaction whose body swallowed a failed statement does not report a commit; requires Docker.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use fundamental_core::Error as _;
use fundamental_db::{DatabaseError, IsolationLevel};
use fundamental_test_containers::sql::SqlSchema;
use sqlx::Row as _;

#[tokio::test]
async fn a_swallowed_statement_error_does_not_commit() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    pool.execute_ddl("CREATE TABLE IF NOT EXISTS booked (id INT PRIMARY KEY)")
        .await
        .expect("the table");
    let runs = Arc::new(AtomicU32::new(0));

    let outcome = pool
        .transaction(IsolationLevel::ReadCommitted, {
            let runs = Arc::clone(&runs);
            async move |tx| {
                runs.fetch_add(1, Ordering::SeqCst);
                tx.execute("INSERT INTO booked (id) VALUES (1)").await?;
                let swallowed = tx.execute("SELECT 1 / 0").await;
                assert!(swallowed.is_err(), "the division must fail");
                Ok::<_, DatabaseError>(())
            }
        })
        .await;

    let error = outcome.expect_err("an aborted transaction reported a commit");
    assert_eq!(error.error_code(), "SQL_019", "got {error:?}");
    assert!(
        !error.is_retryable(),
        "a division by zero is not a conflict"
    );
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    let row = pool
        .fetch_one("SELECT count(*) FROM booked")
        .await
        .expect("the count");
    let booked: i64 = row.try_get(0).expect("a count");
    assert_eq!(booked, 0, "a write from the aborted transaction is visible");
}

#[tokio::test]
async fn a_failure_rolled_back_to_its_savepoint_still_commits() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    pool.execute_ddl("CREATE TABLE IF NOT EXISTS kept (id INT PRIMARY KEY)")
        .await
        .expect("the table");

    pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
        tx.execute("INSERT INTO kept (id) VALUES (1)").await?;
        tx.execute_savepoint("risky").await?;
        assert!(tx.execute("SELECT 1 / 0").await.is_err());
        assert!(tx.aborted_by().is_some(), "the failure was not recorded");
        tx.execute_rollback_to_savepoint("risky").await?;
        assert_eq!(tx.aborted_by(), None, "the recovery was not recorded");
        tx.execute("INSERT INTO kept (id) VALUES (2)").await?;
        Ok::<_, DatabaseError>(())
    })
    .await
    .expect("a recovered transaction commits");

    let row = pool
        .fetch_one("SELECT count(*) FROM kept")
        .await
        .expect("the count");
    let kept: i64 = row.try_get(0).expect("a count");
    assert_eq!(kept, 2);
}

#[derive(Clone, Debug, fundamental_macros::Model)]
#[model(sql)]
struct Ledger {
    #[model(id)]
    ledger_id: i64,
}

/// Generated model code runs on the transaction's connection, not through its methods.
#[tokio::test]
async fn a_swallowed_failure_of_a_generated_model_call_does_not_commit() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    pool.execute_ddl("CREATE TABLE IF NOT EXISTS ledgers (ledger_id BIGINT PRIMARY KEY)")
        .await
        .expect("the table");

    let outcome = pool
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            let entry = Ledger { ledger_id: 1 };
            entry.insert(&mut *tx).await?;
            assert!(
                entry.insert(&mut *tx).await.is_err(),
                "the second insert of one key must fail"
            );
            Ok::<_, DatabaseError>(())
        })
        .await;

    let error = outcome.expect_err("an aborted transaction reported a commit");
    assert_eq!(error.error_code(), "SQL_019", "got {error:?}");
    let row = pool
        .fetch_one("SELECT count(*) FROM ledgers")
        .await
        .expect("the count");
    let written: i64 = row.try_get(0).expect("a count");
    assert_eq!(written, 0);
}
