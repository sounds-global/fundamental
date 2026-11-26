//! A migration that fails carries the engine's verdict, not a rendering of it.
//!
//! The conflict is raised with `RAISE … USING ERRCODE` on every attempt, so the exhausted budget is reached deterministically.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use fundamental_core::Error as _;
use fundamental_db::DatabaseError;
use fundamental_db::DatabaseMetrics;
use fundamental_db::migration::{Lints, MigrationGuard};
use fundamental_sql::runner::SqlMigrator;
use fundamental_sql::{SqlConfig, SqlError, SqlPool};
use fundamental_test_containers::sql::SqlSchema;

fn guard() -> MigrationGuard {
    MigrationGuard::new(1_000, Lints::default(), &[])
}

/// Applied through the pool, as the statement splitter divides a `plpgsql` body on `;`.
const RAISE_A_CONFLICT: &str = "CREATE OR REPLACE FUNCTION raise_a_conflict() RETURNS void AS $fn$ \
     BEGIN RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'forced serialization conflict'; \
     END $fn$ LANGUAGE plpgsql";

/// A pool whose twenty-four migration attempts cost milliseconds; the count is the runner's, the delays configuration.
async fn fast_retry_pool(schema: &SqlSchema) -> (SqlPool, Arc<RecordedRetries>) {
    let config = SqlConfig::development(schema.url())
        .default_schema(schema.schema().clone())
        .retry_delays(Duration::from_millis(1), Duration::from_millis(5));
    let retries = Arc::new(RecordedRetries::default());
    let pool = SqlPool::connect(&config)
        .await
        .expect("a pool against the minted database")
        .with_metrics(Arc::clone(&retries) as Arc<dyn DatabaseMetrics>);
    (pool, retries)
}

/// Creates the ledger, then clears the recorder, so a genuine `40001` during setup is not counted.
async fn ledger_created_and_counter_cleared(pool: &SqlPool, retries: &RecordedRetries) {
    SqlMigrator::new(guard())
        .apply_all(pool, &[("0000_ledger", "SELECT 1;")])
        .await
        .expect("a migration that does nothing still creates the ledger");
    retries.clear();
}

#[derive(Default)]
struct RecordedRetries {
    seen: Mutex<Vec<String>>,
    exhausted: Mutex<Vec<String>>,
}

impl RecordedRetries {
    fn seen(&self) -> Vec<String> {
        self.seen.lock().expect("the retry log").clone()
    }

    fn exhausted(&self) -> Vec<String> {
        self.exhausted.lock().expect("the exhaustion log").clone()
    }

    /// Forgets what has been recorded, so setup's own retries are not counted.
    fn clear(&self) {
        self.seen.lock().expect("the retry log").clear();
        self.exhausted.lock().expect("the exhaustion log").clear();
    }
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

    fn record_retry_exhausted(&self, sqlstate: &str) {
        self.exhausted
            .lock()
            .expect("the exhaustion log")
            .push(sqlstate.to_owned());
    }
}

#[tokio::test]
async fn an_exhausted_retry_still_says_the_engine_called_it_a_conflict() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let (pool, retries) = fast_retry_pool(&schema).await;
    // `execute_ddl` and `OR REPLACE`: a catalog write that contends, made safe to re-issue.
    pool.execute_ddl(RAISE_A_CONFLICT)
        .await
        .expect("the conflict probe");

    // Provisioning's own retries must not be in the exact count below.
    ledger_created_and_counter_cleared(&pool, &retries).await;

    let error = SqlMigrator::new(guard())
        .apply_all(&pool, &[("0001_conflict", "SELECT raise_a_conflict();")])
        .await
        .expect_err("a migration that raises 40001 on every attempt must fail");

    let SqlError::MigrationFailed { name, cause, .. } = &error else {
        panic!("expected MigrationFailed, got {error:?}");
    };
    assert_eq!(
        name, "0001_conflict",
        "the failure must still name the migration"
    );
    assert_eq!(
        cause.sqlstate(),
        Some("40001"),
        "the engine's code must survive the runner — it is the retry metric's label"
    );
    assert_eq!(
        error.error_code(),
        "SQL_011",
        "and the code stays the migration's own"
    );

    // The runner's view, a `dyn Error`'s, and a deploy's two `AnyError` hops away must agree.
    assert!(
        error.is_retryable(),
        "the concrete type must ask to be retried"
    );
    assert!(
        fundamental_core::Error::is_retryable(&error),
        "and so must the trait method, which is the one visible through `dyn Error`"
    );
    assert!(
        DatabaseError::from(error).is_retryable(),
        "and so must it behind `AnyError`, which is the shape a deploy's migrator returns"
    );

    // Without this the test would pass on a failure that never reached the retry loop.
    let seen = retries.seen();
    assert_eq!(
        seen.len(),
        23,
        "MIGRATION_ATTEMPTS is 24, so a failure that is retryable every time must be retried \
         23 times before the last attempt reaches the caller: {seen:?}"
    );
    assert!(
        seen.iter().all(|code| code == "40001"),
        "and every retry must name the class it retried rather than `unknown`: {seen:?}"
    );

    // Counted once per spent budget, never per attempt.
    assert_eq!(
        retries.exhausted(),
        vec!["40001".to_owned()],
        "a spent budget must be counted exactly once, under the class that spent it"
    );
}

/// The control, without which every assertion above is satisfied by a constant `true`.
#[tokio::test]
async fn a_permanent_failure_says_so_through_all_three_of_them() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let (pool, retries) = fast_retry_pool(&schema).await;

    ledger_created_and_counter_cleared(&pool, &retries).await;

    let error = SqlMigrator::new(guard())
        .apply_all(
            &pool,
            &[(
                "0001_missing",
                "SELECT * FROM a_relation_that_does_not_exist;",
            )],
        )
        .await
        .expect_err("a migration naming a missing relation must fail");

    let SqlError::MigrationFailed { cause, .. } = &error else {
        panic!("expected MigrationFailed, got {error:?}");
    };
    assert_eq!(
        cause.sqlstate(),
        None,
        "42P01 is not in the retryable set and carries no class this reports"
    );
    assert!(
        !error.is_retryable(),
        "a migration written against a relation that is not there is not fixed by running it again"
    );
    assert!(!fundamental_core::Error::is_retryable(&error));
    assert!(!DatabaseError::from(error).is_retryable());
    assert!(
        retries.seen().is_empty(),
        "and a permanent failure must not have been retried at all: {:?}",
        retries.seen()
    );
    assert!(
        retries.exhausted().is_empty(),
        "nor counted as a budget spent — nothing was retried, so nothing ran out: {:?}",
        retries.exhausted()
    );
}
