//! `find_for_update_unless_held` locks or reports a held row without waiting; requires Docker.

use std::sync::Arc;
use std::time::{Duration, Instant};

use fundamental_core::Error as _;
use fundamental_db::{
    DatabaseError, IsolationLevel, RepositoryTransaction as _, Transaction as _,
    TransactionOptions, Transactional as _,
};
use fundamental_macros::Model;
use fundamental_sql::{RowLockOutcome, SqlPool};
use fundamental_test_containers::sql::SqlSchema;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct ContestedRow {
    #[model(id)]
    id: Uuid,
    label: String,
}

const PROMPT: Duration = Duration::from_secs(3);

async fn table() -> (SqlSchema, SqlPool, ContestedRow) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS contested_rows (id UUID NOT NULL, label TEXT NOT NULL, \
         PRIMARY KEY (id HASH))",
    )
    .await
    .expect("the table exists");
    let row = ContestedRow {
        id: Uuid::new_v4(),
        label: "contested".to_owned(),
    };
    let written = row.clone();
    pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
        tx.insert(&written).await?;
        Ok::<_, DatabaseError>(())
    })
    .await
    .expect("the row is written");
    (schema, pool, row)
}

async fn hold(
    pool: &SqlPool,
    row: &ContestedRow,
) -> (Arc<Notify>, JoinHandle<Result<(), DatabaseError>>) {
    let (held, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let holder = tokio::spawn({
        let (pool, id) = (pool.clone(), row.id);
        let (held, release) = (Arc::clone(&held), Arc::clone(&release));
        async move {
            pool.transaction(IsolationLevel::ReadCommitted, async move |tx| {
                tx.find_for_update::<ContestedRow>(&id).await?;
                held.notify_one();
                release.notified().await;
                Ok::<_, DatabaseError>(())
            })
            .await
        }
    });
    held.notified().await;
    (release, holder)
}

async fn ask(
    pool: &SqlPool,
    level: IsolationLevel,
    id: Uuid,
) -> Result<RowLockOutcome<ContestedRow>, DatabaseError> {
    let mut tx = pool
        .begin_with(TransactionOptions::new(level))
        .await
        .expect("begins");
    let outcome = timeout(PROMPT, tx.find_for_update_unless_held::<ContestedRow>(&id))
        .await
        .expect("asking for a lock never waits");
    tx.rollback().await.expect("rolls back");
    outcome
}

#[tokio::test]
async fn a_free_row_is_locked_at_read_committed_and_repeatable_read() {
    let (_schema, pool, row) = table().await;
    for level in [
        IsolationLevel::ReadCommitted,
        IsolationLevel::RepeatableRead,
    ] {
        assert_eq!(
            ask(&pool, level, row.id).await.expect("asks"),
            RowLockOutcome::Locked(row.clone()),
            "at {level:?}"
        );
    }
}

#[tokio::test]
async fn a_missing_row_is_absent() {
    let (_schema, pool, _row) = table().await;
    assert_eq!(
        ask(&pool, IsolationLevel::ReadCommitted, Uuid::new_v4())
            .await
            .expect("asks"),
        RowLockOutcome::Absent
    );
}

#[tokio::test]
async fn a_held_row_is_reported_without_waiting() {
    let (_schema, pool, row) = table().await;
    let (release, holder) = hold(&pool, &row).await;

    for level in [
        IsolationLevel::ReadCommitted,
        IsolationLevel::RepeatableRead,
    ] {
        let started = Instant::now();
        assert_eq!(
            ask(&pool, level, row.id).await.expect("asks"),
            RowLockOutcome::HeldElsewhere(row.clone()),
            "at {level:?}"
        );
        assert!(
            started.elapsed() < PROMPT,
            "a held row took {:?} to report at {level:?}",
            started.elapsed()
        );
    }

    release.notify_one();
    holder
        .await
        .expect("the holder ran")
        .expect("the holder commits after being asked about");
    assert!(
        ask(&pool, IsolationLevel::ReadCommitted, row.id)
            .await
            .expect("asks")
            .is_locked(),
        "a released row is locked by the next to ask"
    );
}

#[tokio::test]
async fn a_lock_taken_this_way_is_held_against_the_next() {
    let (_schema, pool, row) = table().await;
    let mut first = pool
        .begin_with(TransactionOptions::new(IsolationLevel::ReadCommitted))
        .await
        .expect("begins");
    assert!(
        first
            .find_for_update_unless_held::<ContestedRow>(&row.id)
            .await
            .expect("asks")
            .is_locked()
    );
    assert_eq!(
        ask(&pool, IsolationLevel::ReadCommitted, row.id)
            .await
            .expect("asks"),
        RowLockOutcome::HeldElsewhere(row.clone())
    );
    first.commit().await.expect("commits");
}

#[tokio::test]
async fn serializable_is_refused_before_it_can_wait() {
    let (_schema, pool, row) = table().await;
    let (release, holder) = hold(&pool, &row).await;

    let refused = ask(&pool, IsolationLevel::Serializable, row.id)
        .await
        .expect_err("serializable would wait, so it is refused");
    assert!(
        matches!(refused, DatabaseError::Configuration { .. }),
        "{refused:?}"
    );
    assert_eq!(refused.error_code(), "DB_009");

    release.notify_one();
    holder.await.expect("the holder ran").expect("commits");
}
