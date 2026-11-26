//! A migration that fails in production says where, blocks every later apply, and is settled by one recorded act.
//!
//! Step 2 of 3 outlasts a two-second statement timeout; the retry runs on a pool without one.

use std::time::Duration;

use fundamental_core::Error as _;
use fundamental_db::migration::{
    LedgerError, LedgerState, Level, Lint, Lints, MigrationGuard, Resolution, Resolve,
};
use fundamental_sql::runner::SqlMigrator;
use fundamental_sql::{SqlConfig, SqlError, SqlPool};
use fundamental_test_containers::sql::SqlSchema;

fn guard() -> MigrationGuard {
    // A step that copies rows into a table it did not create is a fixture here, not a backfill.
    MigrationGuard::new(
        1_000,
        Lints::default().with(Lint::Backfill, Level::Allow),
        &[],
    )
}

const ORDERS: &str = "CREATE TABLE IF NOT EXISTS orders (id UUID NOT NULL, note TEXT, \
     PRIMARY KEY (id HASH));";
const INDEX_ORDERS: &str = "ALTER TABLE orders ADD COLUMN IF NOT EXISTS customer_id UUID;\n\
     SELECT pg_sleep(4);\n\
     CREATE INDEX IF NOT EXISTS orders_by_customer ON orders (customer_id HASH);";
const PAID_AT: &str = "ALTER TABLE orders ADD COLUMN IF NOT EXISTS paid_at TIMESTAMPTZ;";

fn files() -> [(&'static str, &'static str); 3] {
    [
        ("0001_orders", ORDERS),
        ("0002_index_orders", INDEX_ORDERS),
        ("0003_orders_paid_at", PAID_AT),
    ]
}

async fn pool(schema: &SqlSchema, timeout: Option<Duration>) -> SqlPool {
    let config = SqlConfig::development(schema.url())
        .default_schema(schema.schema().clone())
        .statement_timeout(timeout);
    SqlPool::connect(&config).await.expect("a pool")
}

fn operator() -> String {
    ["SUDO_USER", "USER"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .find(|value| !value.trim().is_empty())
        .expect("the test runs as somebody")
}

#[tokio::test]
async fn a_failed_migration_is_resolved_once() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let at_peak = pool(&schema, Some(Duration::from_secs(2))).await;
    let at_night = pool(&schema, None).await;
    let migrator = SqlMigrator::new(guard());
    migrator
        .apply_all(&at_night, &files()[..1])
        .await
        .expect("the table, and the ledger");

    fails_at_step_two(&migrator, &at_peak).await;
    status_says_where(&migrator, &at_night).await;
    every_later_apply_is_refused(&migrator, &at_night).await;
    the_retry_finishes_it_and_is_recorded(&migrator, &at_night).await;
}

async fn fails_at_step_two(migrator: &SqlMigrator, at_peak: &SqlPool) {
    let error = migrator
        .apply_all(at_peak, &files())
        .await
        .expect_err("step 2 times out");
    let SqlError::MigrationFailed {
        name,
        step,
        total,
        statement,
        ..
    } = &error
    else {
        panic!("expected MigrationFailed, got {error:?}");
    };
    assert_eq!((name.as_str(), *step, *total), ("0002_index_orders", 2, 3));
    assert_eq!(statement, "SELECT pg_sleep(4);");
}

async fn status_says_where(migrator: &SqlMigrator, at_night: &SqlPool) {
    let ledger = migrator
        .ledger(at_night, &files())
        .await
        .expect("the ledger reads");
    let states: Vec<&LedgerState> = ledger.entries().iter().map(|entry| entry.state()).collect();
    assert!(matches!(
        states[0],
        LedgerState::Applied { resolved: None, .. }
    ));
    let LedgerState::Failed(failed) = states[1] else {
        panic!("expected the second failed, got {states:?}");
    };
    assert_eq!((failed.step, failed.total), (2, 3));
    assert_eq!(failed.statement, "SELECT pg_sleep(4);");
    assert_eq!(failed.code.as_deref(), Some("57014"), "{failed:?}");
    assert!(failed.error.contains("statement timeout"), "{failed:?}");
    assert!(!failed.answer_lost, "a timeout is an answer: {failed:?}");
    assert!(
        matches!(states[2], LedgerState::Blocked { behind } if behind == "0002_index_orders"),
        "{states:?}"
    );
    let text = ledger.to_string();
    assert!(
        text.contains("0002_index_orders     failed at step 2 of 3"),
        "{text}"
    );
    assert!(text.contains("57014: "), "{text}");
}

async fn every_later_apply_is_refused(migrator: &SqlMigrator, at_night: &SqlPool) {
    let refused = migrator
        .apply_all(at_night, &files())
        .await
        .expect_err("an unresolved migration blocks the apply");
    assert!(
        matches!(
            &refused,
            SqlError::Ledger(LedgerError::Unresolved { name, step: 2, total: 3 })
                if name == "0002_index_orders"
        ),
        "{refused:?}"
    );
    assert_eq!(refused.error_code(), "MIGRATE_016");
}

async fn the_retry_finishes_it_and_is_recorded(migrator: &SqlMigrator, at_night: &SqlPool) {
    let resolution = Resolution::new("timeout at peak; rerun at night").expect("a resolution");
    let settled = migrator
        .retry(at_night, ("0002_index_orders", INDEX_ORDERS), &resolution)
        .await
        .expect("the retry finishes it");
    assert_eq!((settled.step, settled.total), (2, 3));

    let ledger = migrator
        .ledger(at_night, &files())
        .await
        .expect("the ledger reads");
    let LedgerState::Applied {
        resolved: Some(resolved),
        ..
    } = ledger.entries()[1].state()
    else {
        panic!("expected the second applied and resolved, got {ledger:?}");
    };
    assert_eq!(resolved.act, Resolve::Retry);
    assert_eq!(resolved.by, operator());
    assert_eq!(resolved.reason, "timeout at peak; rerun at night");
    assert_eq!(resolved.step, 2);
    assert_eq!(resolved.code.as_deref(), Some("57014"));
    assert!(
        resolved
            .login
            .as_deref()
            .is_some_and(|login| !login.is_empty()),
        "the database login is recorded: {resolved:?}"
    );
    assert!(matches!(ledger.entries()[2].state(), LedgerState::Pending));

    the_next_apply_carries_on(migrator, at_night, &resolution).await;
}

async fn the_next_apply_carries_on(
    migrator: &SqlMigrator,
    at_night: &SqlPool,
    resolution: &Resolution,
) {
    let indexes = at_night
        .fetch_all(
            "SELECT indexname::text FROM pg_indexes \
             WHERE schemaname = current_schema() AND indexname = 'orders_by_customer'",
        )
        .await
        .expect("the catalog reads");
    assert_eq!(indexes.len(), 1, "step 3 made the index");
    let applied = migrator
        .apply_all(at_night, &files())
        .await
        .expect("nothing is in the way");
    assert_eq!(applied, ["0003_orders_paid_at"]);

    let nothing = migrator
        .retry(at_night, ("0002_index_orders", INDEX_ORDERS), resolution)
        .await
        .expect_err("it is applied");
    assert!(
        matches!(
            nothing,
            SqlError::Ledger(LedgerError::NothingToResolve { .. })
        ),
        "{nothing:?}"
    );
}

#[tokio::test]
async fn a_step_whose_answer_was_lost_is_retried_only_if_it_can_run_again() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = pool(&schema, None).await;
    let migrator = SqlMigrator::new(guard());
    migrator
        .apply_all(&pool, &[("0001_orders", ORDERS)])
        .await
        .expect("the table");

    const RENAME: &str = "ALTER TABLE orders RENAME COLUMN note TO memo;";
    pool.execute(&format!(
        "INSERT INTO {}._fundamental_migration_steps \
         (name, step, total, statement_hash, state, answer_lost, at) \
         VALUES ('0002_rename_note', 1, 1, 'unused', 'running', false, now())",
        schema.schema()
    ))
    .await
    .expect("the stopped step");

    let resolution = Resolution::new("the deploy host rebooted").expect("a resolution");
    let refused = migrator
        .retry(&pool, ("0002_rename_note", RENAME), &resolution)
        .await
        .expect_err("a rename that may have applied cannot run again");
    assert!(
        matches!(
            &refused,
            SqlError::Ledger(LedgerError::NotRerunnable { name, step: 1, total: 1, .. })
                if name == "0002_rename_note"
        ),
        "{refused:?}"
    );
    assert_eq!(refused.error_code(), "MIGRATE_018");

    let ledger = migrator
        .ledger(
            &pool,
            &[("0001_orders", ORDERS), ("0002_rename_note", RENAME)],
        )
        .await
        .expect("the ledger reads");
    let LedgerState::Failed(failed) = ledger.entries()[1].state() else {
        panic!("the stopped step is recorded as failed: {ledger:?}");
    };
    assert!(failed.answer_lost, "{failed:?}");
}

/// The retry runs on a new connection, and the step after a `SET` relies on it.
#[tokio::test]
async fn a_retry_makes_the_settings_of_the_steps_before_it_again() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let at_night = pool(&schema, None).await;
    let migrator = SqlMigrator::new(guard());
    let marks = [(
        "0001_marks",
        "CREATE TABLE IF NOT EXISTS marks (mark TEXT NOT NULL, PRIMARY KEY (mark HASH));",
    )];
    migrator
        .apply_all(&at_night, &marks)
        .await
        .expect("the table");
    let files = [
        marks[0],
        (
            "0002_mark",
            "SET fundamental.mark = 'set';\n\
             INSERT INTO marks (mark) SELECT current_setting('fundamental.mark') FROM ready;",
        ),
    ];
    migrator
        .apply_all(&at_night, &files)
        .await
        .expect_err("step 2 of 2 reads a table that is not there yet");

    at_night
        .execute_ddl("CREATE TABLE ready (id INT NOT NULL, PRIMARY KEY (id HASH))")
        .await
        .expect("the table step 2 reads, by hand");
    at_night
        .execute_ddl("INSERT INTO ready (id) VALUES (1)")
        .await
        .expect("its row");
    let resolution = Resolution::new("made the table it reads").expect("a resolution");
    migrator
        .retry(&at_night, files[1], &resolution)
        .await
        .expect("the setting is made again, so step 2 reads it");
    let mark: String = sqlx::query_scalar("SELECT mark FROM marks")
        .fetch_one(&at_night.inner())
        .await
        .expect("the mark");
    assert_eq!(mark, "set");
}
