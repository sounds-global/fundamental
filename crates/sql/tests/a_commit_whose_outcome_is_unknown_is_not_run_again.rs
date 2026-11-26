//! A transaction whose COMMIT lost its answer is never run a second time; requires Docker.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use fundamental_core::Error as _;
use fundamental_db::{DatabaseError, IsolationLevel};
use fundamental_test_containers::sql::SqlSchema;
use sqlx::Row as _;

#[tokio::test]
async fn a_commit_whose_outcome_is_unknown_is_not_run_again() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    let runs = Arc::new(AtomicU32::new(0));

    let outcome = schema
        .pool()
        .transaction(IsolationLevel::Serializable, {
            let runs = Arc::clone(&runs);
            let pool = pool.clone();
            async move |tx| {
                runs.fetch_add(1, Ordering::SeqCst);
                let row = tx.fetch_one("SELECT pg_backend_pid()").await?;
                let pid: i32 = row
                    .try_get(0)
                    .map_err(|e| DatabaseError::internal(e.to_string()))?;
                pool.execute(&format!("SELECT pg_terminate_backend({pid})"))
                    .await?;
                Ok::<_, DatabaseError>(())
            }
        })
        .await;

    let error = outcome.expect_err("the commit cannot succeed on a terminated backend");
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "the body ran again after a commit whose outcome nobody knows: {error}"
    );
    assert_eq!(error.error_code(), "DB_012", "got {error:?}");
    assert!(!error.is_retryable());
}
