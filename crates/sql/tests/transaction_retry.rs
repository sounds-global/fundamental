//! A serialization conflict is retried rather than handed to the caller.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fundamental_db::DatabaseError;
use fundamental_db::DatabaseMetrics;
use fundamental_db::IsolationLevel;
use fundamental_macros::Error;
use fundamental_sql::{SqlConfig, SqlError, SqlPool};
use fundamental_test_containers::sql::SqlSchema;
use tokio::time::timeout;
use tracing::warn;

/// How long a writer waits at the contention gate; reaching it means the fixture did not arrange the conflict.
const GATE_BUDGET: Duration = Duration::from_secs(30);

#[derive(Debug, Error)]
enum ProbeError {
    #[error(
        code = "PROBE_001",
        message = "The row is busy. Please try again.",
        status = 503,
        retryable = true
    )]
    Contended,

    #[error(code = "PROBE_002", message = "That cannot be done.", status = 400)]
    Permanent,

    #[error(code = "PROBE_003", message = "Storage failed.", status = 500)]
    Store(String),
}

impl From<DatabaseError> for ProbeError {
    fn from(error: DatabaseError) -> Self {
        Self::Store(error.to_string())
    }
}

impl From<SqlError> for ProbeError {
    fn from(error: SqlError) -> Self {
        Self::Store(error.to_string())
    }
}
async fn probe_table(pool: &SqlPool) {
    // `INT` is `int4`, so the `i64` decode needs the width stated; `execute_ddl` rides out catalog contention.
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS retry_probe (id BIGINT PRIMARY KEY, n BIGINT NOT NULL)",
    )
    .await
    .expect("the probe table");
    pool.with_retry(|| async {
        pool.execute(
            "INSERT INTO retry_probe (id, n) VALUES (1, 0) \
             ON CONFLICT (id) DO UPDATE SET n = EXCLUDED.n",
        )
        .await
    })
    .await
    .expect("a row to contend over");
}

#[tokio::test]
async fn a_retryable_error_runs_the_transaction_again() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool();
    probe_table(pool).await;

    let attempts = Arc::new(AtomicU32::new(0));
    let seen = Arc::clone(&attempts);

    let result: Result<u32, ProbeError> = pool
        .transaction(IsolationLevel::Serializable, async |_tx| {
            let attempt = seen.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt < 3 {
                return Err(ProbeError::Contended);
            }
            Ok(attempt)
        })
        .await;

    assert_eq!(
        result.expect("the third attempt commits"),
        3,
        "the closure must run again after a retryable error"
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn a_permanent_error_is_not_retried() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool();
    probe_table(pool).await;

    let attempts = Arc::new(AtomicU32::new(0));
    let seen = Arc::clone(&attempts);

    let result: Result<(), ProbeError> = pool
        .transaction(IsolationLevel::Serializable, async |_tx| {
            seen.fetch_add(1, Ordering::SeqCst);
            Err(ProbeError::Permanent)
        })
        .await;

    assert!(result.is_err());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "a permanent error must not be tried again"
    );
}

#[tokio::test]
async fn the_budget_is_spent_and_the_error_reaches_the_caller() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool();
    probe_table(pool).await;

    let attempts = Arc::new(AtomicU32::new(0));
    let seen = Arc::clone(&attempts);

    let result: Result<(), ProbeError> = pool
        .transaction(IsolationLevel::Serializable, async |_tx| {
            seen.fetch_add(1, Ordering::SeqCst);
            Err(ProbeError::Contended)
        })
        .await;

    assert!(result.is_err(), "the conflict must reach the caller");
    let tried = attempts.load(Ordering::SeqCst);
    assert!(
        tried > 1 && tried <= 16,
        "bounded retries: tried {tried} times"
    );
}

#[tokio::test]
async fn the_committed_attempt_is_the_one_that_persists() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool();
    probe_table(pool).await;

    let attempts = Arc::new(AtomicU32::new(0));
    let seen = Arc::clone(&attempts);

    let result: Result<(), ProbeError> = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            let attempt = seen.fetch_add(1, Ordering::SeqCst) + 1;
            tx.execute("UPDATE retry_probe SET n = n + 1 WHERE id = 1")
                .await?;
            if attempt < 2 {
                return Err(ProbeError::Contended);
            }
            Ok(())
        })
        .await;
    result.expect("the second attempt commits");

    let rows = pool
        .fetch_all("SELECT n FROM retry_probe WHERE id = 1")
        .await
        .expect("read back");
    let n: i64 = sqlx::Row::try_get(&rows[0], "n").expect("the counter");
    assert_eq!(
        n, 1,
        "the rolled-back attempt must not count; only the committed one persists"
    );
}

#[derive(Default)]
struct RecordedRetries {
    seen: Mutex<Vec<String>>,
}

impl DatabaseMetrics for RecordedRetries {
    fn record_query(&self, _table: &str, _operation: &str, _elapsed: Duration, _ok: bool) {}
    fn record_connection_acquire(&self, _elapsed: Duration, _ok: bool) {}
    fn record_slow_query(&self, _table: &str, _operation: &str, _elapsed: Duration) {}
    fn record_transaction(&self, _committed: bool, _elapsed: Duration) {}

    fn record_retry(&self, sqlstate: &str) {
        self.seen
            .lock()
            .expect("the retry log")
            .push(sqlstate.to_owned());
    }
}

/// Runs `writers` concurrent serializable increments of one row; returns the counter and the retries' SQLSTATEs.
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn contend(schema: &SqlSchema, writers: i64, flatten: bool) -> (i64, Vec<String>) {
    let pool = schema.pool();
    probe_table(pool).await;

    // Stated: the default budget does not cover even four-way contention on one row.
    let config = SqlConfig::development(schema.url())
        .default_schema(schema.schema().clone())
        .max_retries(24)
        .retry_delays(Duration::from_millis(5), Duration::from_millis(250));
    let metrics = Arc::new(RecordedRetries::default());
    let instrumented = SqlPool::connect(&config)
        .await
        .expect("a pool with a stated retry budget")
        .with_metrics(Arc::clone(&metrics) as Arc<_>);

    // Every writer reads the row, then waits here for all the others, so the conflict is arranged, not hoped for.
    let gate = Arc::new(tokio::sync::Barrier::new(
        usize::try_from(writers).expect("a small writer count"),
    ));

    let mut tasks = Vec::new();
    for _ in 0..writers {
        let pool = instrumented.clone();
        let gate = Arc::clone(&gate);
        tasks.push(tokio::spawn(async move {
            // Only the first attempt waits: a retry has nobody left to meet.
            let arrived = AtomicBool::new(false);
            pool.transaction(IsolationLevel::Serializable, async |tx| {
                // Not `FOR UPDATE`: a row lock makes writers queue, and a queued writer never conflicts.
                let rows = tx
                    .fetch_all("SELECT n FROM retry_probe WHERE id = 1")
                    .await
                    .map_err(|error| {
                        // A call site that flattens the engine's error into a string; the retry must survive it.
                        if flatten {
                            ProbeError::Permanent
                        } else {
                            ProbeError::Store(error.to_string())
                        }
                    })?;
                let n: i64 = sqlx::Row::try_get(&rows[0], "n").expect("the counter");
                if !arrived.swap(true, Ordering::Relaxed) {
                    // Bounded, and reported when it elapses, since the conflict was then never arranged.
                    if timeout(GATE_BUDGET, gate.wait()).await.is_err() {
                        warn!(
                            writers,
                            budget_secs = GATE_BUDGET.as_secs(),
                            "the contention gate elapsed: not every writer reached it, so the \
                             conflict under test was not arranged and this run proves nothing \
                             about the retry"
                        );
                    }
                }
                tx.execute(&format!(
                    "UPDATE retry_probe SET n = {} WHERE id = 1",
                    n + 1
                ))
                .await
                .map_err(|error| {
                    if flatten {
                        ProbeError::Permanent
                    } else {
                        ProbeError::Store(error.to_string())
                    }
                })?;
                Ok::<(), ProbeError>(())
            })
            .await
        }));
    }
    for task in tasks {
        task.await.expect("the writer task").expect("the writer");
    }

    let rows = pool
        .fetch_all("SELECT n FROM retry_probe WHERE id = 1")
        .await
        .expect("read back");
    let total: i64 = sqlx::Row::try_get(&rows[0], "n").expect("the counter");
    let seen = metrics.seen.lock().expect("the retry log").clone();
    (total, seen)
}

#[tokio::test]
async fn every_contending_writer_commits() {
    let schema = SqlSchema::mint().await.expect("a test schema");

    let (total, _) = contend(&schema, 4, false).await;

    assert_eq!(
        total, 4,
        "every writer's increment must survive the contention"
    );
}

/// The retry decision comes off the transaction, so mapping the error inside the closure cannot defeat it.
#[tokio::test]
async fn a_conflict_is_retried_through_a_closure_that_threw_it_away() {
    let schema = SqlSchema::mint().await.expect("a test schema");

    let (total, _) = contend(&schema, 4, true).await;

    assert_eq!(
        total, 4,
        "the conflict must be retried even though the closure reported it as permanent"
    );
}

#[tokio::test]
async fn a_retry_carries_the_sqlstate_the_engine_reported() {
    let schema = SqlSchema::mint().await.expect("a test schema");

    let (_, seen) = contend(&schema, 4, false).await;

    assert!(
        !seen.is_empty(),
        "four writers on one row must contend at least once"
    );
    assert!(
        seen.iter().all(|code| code == "40001" || code == "40P01"),
        "every retry must name the class it retried, not `unknown`: {seen:?}"
    );
}
