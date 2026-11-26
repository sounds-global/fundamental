//! `TransactionOptions::timeout` bounds a transaction's row-lock waits; requires Docker.

use std::sync::Arc;
use std::time::{Duration, Instant};

use fundamental_db::{
    DatabaseError, IsolationLevel, RepositoryTransaction as _, Transaction as _,
    TransactionOptions, Transactional as _,
};
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;
use tokio::sync::Notify;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct WaitedRow {
    #[model(id)]
    id: Uuid,
    label: String,
}

/// How long a statement that was going to wait is watched waiting.
const SETTLE: Duration = Duration::from_millis(1_500);

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn a_transaction_timeout_bounds_its_lock_waits() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS waited_rows (id UUID NOT NULL, label TEXT NOT NULL, \
         PRIMARY KEY (id HASH))",
    )
    .await
    .expect("the table exists");
    let row = WaitedRow {
        id: Uuid::new_v4(),
        label: "held".to_owned(),
    };
    let written = row.clone();
    pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
        tx.insert(&written).await?;
        Ok::<_, DatabaseError>(())
    })
    .await
    .expect("the row is written");

    let (held, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let holder = tokio::spawn({
        let pool = pool.clone();
        let (held, release) = (Arc::clone(&held), Arc::clone(&release));
        async move {
            pool.transaction(IsolationLevel::ReadCommitted, async move |tx| {
                tx.find_for_update::<WaitedRow>(&row.id).await?;
                held.notify_one();
                release.notified().await;
                Ok::<_, DatabaseError>(())
            })
            .await
        }
    });
    held.notified().await;

    let mut bounded = pool
        .begin_with(TransactionOptions::new(IsolationLevel::ReadCommitted).timeout(100))
        .await
        .expect("begins");
    let started = Instant::now();
    let waited = timeout(
        Duration::from_secs(10),
        bounded.find_for_update::<WaitedRow>(&row.id),
    )
    .await
    .expect("a lock wait under a 100 ms timeout ends");
    assert!(waited.is_err(), "the lock was granted while it was held");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "a 100 ms lock timeout waited {:?}",
        started.elapsed()
    );
    bounded.rollback().await.expect("rolls back");

    let unbounded = tokio::spawn({
        let pool = pool.clone();
        async move {
            pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
                tx.find_for_update::<WaitedRow>(&row.id).await
            })
            .await
        }
    });
    sleep(SETTLE).await;
    assert!(
        !unbounded.is_finished(),
        "a transaction begun without a timeout stopped waiting"
    );
    release.notify_one();
    holder
        .await
        .expect("the holder ran")
        .expect("the holder commits");
    let locked = unbounded
        .await
        .expect("the waiter ran")
        .expect("the waiter commits once the holder ends");
    assert!(locked.is_some());
}
