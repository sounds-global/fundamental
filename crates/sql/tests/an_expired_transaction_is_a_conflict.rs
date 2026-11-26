//! A transaction the engine expired, reported untagged as `XX000`, is retried, and the rest of `internal_error` is not.
//!
//! The condition is raised with `RAISE … USING ERRCODE`, as provoking it needs a process-wide
//! gflag on the shared node. Reproducible on a throwaway node:
//!
//! ```text
//! --tserver_flags="transaction_heartbeat_usec=500000,transaction_max_missed_heartbeat_periods=0.002"
//! BEGIN ISOLATION LEVEL SERIALIZABLE; INSERT …; COMMIT;
//! ERROR:  XX000: Attempted to commit expired transaction
//! LOCATION:  transaction_coordinator.cc:822
//! ```

use std::sync::atomic::{AtomicU32, Ordering};

use fundamental_core::Error as _;
use fundamental_sql::{SqlConfig, SqlError, SqlPool};
use fundamental_test_containers::sql::SqlSchema;

/// The two wordings this engine has used for one condition.
const WORDINGS: [&str; 2] = [
    "Attempted to commit expired transaction",
    "Commit of expired transaction",
];

/// A capitalisation neither shipped wording uses, so the case fold is asserted.
const SHOUTED: &str = "ATTEMPTED TO COMMIT EXPIRED TRANSACTION";

async fn pool_for(schema: &SqlSchema) -> SqlPool {
    let config = SqlConfig::development(schema.url()).default_schema(schema.schema().clone());
    SqlPool::connect(&config)
        .await
        .expect("a pool against the minted database")
}

async fn engine_reports(pool: &SqlPool, sqlstate: &str, message: &str) -> SqlError {
    let statement = format!(
        "DO $probe$ BEGIN RAISE EXCEPTION USING ERRCODE = '{sqlstate}', \
         MESSAGE = '{message}'; END $probe$"
    );
    pool.execute(&statement)
        .await
        .expect_err("the probe raises")
}

#[tokio::test]
async fn an_expired_transaction_is_classified_as_a_conflict() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = pool_for(&schema).await;

    for wording in WORDINGS {
        let error = engine_reports(&pool, "XX000", wording).await;

        assert!(
            matches!(error, SqlError::TransactionExpired { .. }),
            "{wording:?} should classify as an expired transaction, got {error:?}"
        );
        assert!(
            error.is_retryable(),
            "{wording:?} is an abort the isolation level's contract says to run again"
        );
        assert_eq!(error.error_code(), "SQL_015");
        assert_eq!(error.http_status_code(), 409);
        assert_eq!(error.sqlstate(), Some("XX000"));
        assert!(
            !error.is_serialization_conflict(),
            "it is a conflict, but not the one `40001` names"
        );
    }
}

#[tokio::test]
async fn the_wording_is_recognised_whatever_its_case() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = pool_for(&schema).await;

    let error = engine_reports(&pool, "XX000", SHOUTED).await;

    assert!(
        matches!(error, SqlError::TransactionExpired { .. }),
        "the marker is matched case-insensitively, got {error:?}"
    );
    assert!(error.is_retryable());
}

#[tokio::test]
async fn the_rest_of_internal_error_stays_permanent() {
    // `XX000` is PostgreSQL's catch-all; retrying the class would turn a defect into a hang.
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = pool_for(&schema).await;

    let error = engine_reports(&pool, "XX000", "something else went wrong entirely").await;

    assert!(
        !error.is_retryable(),
        "an unrelated internal error must not be retried: {error:?}"
    );
    assert_eq!(
        error.sqlstate(),
        None,
        "and it must not claim a class the engine did not give it"
    );
}

#[tokio::test]
async fn the_retry_loop_actually_runs_it_again() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = pool_for(&schema).await;

    let attempts = AtomicU32::new(0);
    let error = pool
        .with_retry_budget(3, || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(engine_reports(&pool, "XX000", WORDINGS[0]).await)
        })
        .await
        .expect_err("the probe raises on every attempt, so the budget is spent");

    assert_eq!(
        attempts.load(Ordering::SeqCst),
        3,
        "every attempt in the budget is spent on a retryable conflict"
    );
    assert!(error.is_retryable(), "and the last one still says why");
}
