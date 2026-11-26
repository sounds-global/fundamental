//! `SqlTransaction::open_savepoints`, and what a rollback to a savepoint does to a later row lock; requires Docker.

use std::time::Duration;

use fundamental_db::{
    DatabaseError, IsolationLevel, RepositoryTransaction as _, Transaction as _, Transactional,
};
use fundamental_macros::Model;
use fundamental_sql::{SqlPool, SqlTransaction};
use fundamental_test_containers::sql::SqlSchema;
use tokio::time::timeout;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct HeldRow {
    #[model(id)]
    id: Uuid,
    label: String,
}

async fn with_row() -> (SqlSchema, HeldRow) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    schema
        .pool()
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS held_rows (id UUID NOT NULL, label TEXT NOT NULL, \
             PRIMARY KEY (id HASH))",
        )
        .await
        .expect("the table exists");
    let row = HeldRow {
        id: Uuid::new_v4(),
        label: "held".to_owned(),
    };
    let written = row.clone();
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&written).await?;
            Ok::<_, DatabaseError>(())
        })
        .await
        .expect("the row is written");
    (schema, row)
}

#[tokio::test]
async fn a_savepoint_is_counted_until_it_is_released() {
    let (schema, _row) = with_row().await;
    let mut tx = schema
        .pool()
        .begin(IsolationLevel::ReadCommitted)
        .await
        .expect("a transaction");
    assert_eq!(tx.open_savepoints(), 0);

    tx.execute_savepoint("outer").await.expect("savepoint");
    tx.execute_savepoint("inner").await.expect("savepoint");
    tx.execute_savepoint("inner")
        .await
        .expect("a name used twice");
    assert_eq!(tx.open_savepoints(), 3);

    tx.execute_rollback_to_savepoint("inner")
        .await
        .expect("rollback to the newer inner");
    assert_eq!(
        tx.open_savepoints(),
        3,
        "a rollback leaves its savepoint open"
    );

    tx.execute_release_savepoint("inner")
        .await
        .expect("release the newer inner");
    assert_eq!(tx.open_savepoints(), 2);

    tx.execute_rollback_to_savepoint("outer")
        .await
        .expect("rollback to outer");
    assert_eq!(
        tx.open_savepoints(),
        1,
        "the rollback closed the older inner too"
    );

    tx.execute_release_savepoint("outer")
        .await
        .expect("release");
    assert_eq!(tx.open_savepoints(), 0);

    assert!(tx.execute_release_savepoint("outer").await.is_err());
    assert_eq!(tx.open_savepoints(), 0, "a refused release changes nothing");
}

async fn another_takes_the_lock(pool: &SqlPool, row: &HeldRow, wait: Duration) -> bool {
    let mut other: SqlTransaction = pool
        .begin(IsolationLevel::ReadCommitted)
        .await
        .expect("a second transaction");
    match timeout(wait, other.find_for_update::<HeldRow>(&row.id)).await {
        Ok(taken) => {
            assert_eq!(
                taken.expect("the locked read").as_ref(),
                Some(row),
                "the lock read the row"
            );
            other.rollback().await.expect("rollback");
            true
        }
        Err(_elapsed) => false,
    }
}

/// YugabyteDB releases a lock taken after a savepoint rolled back to, so a lock-resting decision must not sit inside one.
#[tokio::test]
async fn a_rollback_to_savepoint_releases_the_row_locks_taken_after_it() {
    let (schema, row) = with_row().await;
    let pool = schema.pool();

    let mut holder = pool
        .begin(IsolationLevel::ReadCommitted)
        .await
        .expect("the holder's transaction");
    holder
        .execute_savepoint("before_lock")
        .await
        .expect("savepoint");
    let locked = holder
        .find_for_update::<HeldRow>(&row.id)
        .await
        .expect("the holder locks the row");
    assert_eq!(locked.as_ref(), Some(&row));

    assert!(
        !another_takes_the_lock(pool, &row, Duration::from_millis(1500)).await,
        "control: before the rollback the lock is held"
    );

    holder
        .execute_rollback_to_savepoint("before_lock")
        .await
        .expect("rollback to the savepoint");

    assert!(
        another_takes_the_lock(pool, &row, Duration::from_secs(5)).await,
        "the lock taken after the savepoint survived its rollback"
    );

    holder.rollback().await.expect("rollback");
}
