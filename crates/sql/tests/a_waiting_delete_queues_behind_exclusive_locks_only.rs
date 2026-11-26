//! Why a proof read from a row is held `FOR UPDATE`: a `DELETE` waiting on `FOR KEY SHARE` is
//! passed by every new shared locker. Requires Docker.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use fundamental_core::model::SqlDb;
use fundamental_db::{
    DatabaseError, IsolationLevel, QueryBuilder, RepositoryTransaction as _,
    RepositoryTransactionQuery as _,
};
use fundamental_macros::Model;
use fundamental_sql::SqlPool;
use fundamental_test_containers::sql::SqlSchema;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct ProvedRow {
    #[model(id)]
    id: Uuid,
    label: String,
}

const SETTLE: Duration = Duration::from_millis(1_500);

const PROMPT: Duration = Duration::from_secs(10);

async fn with_row() -> (SqlSchema, Uuid) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    schema
        .pool()
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS proved_rows (id UUID NOT NULL, label TEXT NOT NULL, \
             PRIMARY KEY (id HASH))",
        )
        .await
        .expect("the table exists");
    let row = ProvedRow {
        id: Uuid::new_v4(),
        label: "proved".to_owned(),
    };
    let id = row.id;
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&row).await?;
            Ok::<_, DatabaseError>(())
        })
        .await
        .expect("the row is written");
    (schema, id)
}

/// `FOR KEY SHARE`, written out, since the framework offers no shared lock.
async fn key_share(
    tx: &mut fundamental_sql::SqlTransaction,
    id: Uuid,
) -> Result<u64, DatabaseError> {
    tx.execute(&format!(
        "SELECT id FROM proved_rows WHERE id = '{id}' FOR KEY SHARE"
    ))
    .await
    .map_err(DatabaseError::from)
}

fn delete(pool: SqlPool, id: Uuid) -> JoinHandle<Result<u64, DatabaseError>> {
    tokio::spawn(async move {
        pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.delete_where(&QueryBuilder::<ProvedRow, SqlDb>::new().pin_key(id))
                .await
        })
        .await
    })
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_new_shared_locker_passes_a_waiting_delete() {
    let (schema, id) = with_row().await;
    let pool = schema.pool().clone();
    let (held, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let holder = tokio::spawn({
        let pool = pool.clone();
        let (held, release) = (Arc::clone(&held), Arc::clone(&release));
        async move {
            pool.transaction(IsolationLevel::ReadCommitted, async move |tx| {
                key_share(tx, id).await?;
                held.notify_one();
                release.notified().await;
                Ok::<_, DatabaseError>(())
            })
            .await
        }
    });
    held.notified().await;
    let deleter = delete(pool.clone(), id);
    sleep(SETTLE).await;
    assert!(
        !deleter.is_finished(),
        "the delete did not wait on the share"
    );

    let passed = timeout(
        PROMPT,
        pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
            key_share(tx, id).await
        }),
    )
    .await
    .expect("a second shared locker is granted at once, ahead of the waiting delete")
    .expect("the second locker commits");
    assert_eq!(passed, 1, "the second locker found the row");
    assert!(
        !deleter.is_finished(),
        "the delete went through while a share was held"
    );

    release.notify_one();
    holder
        .await
        .expect("the holder ran")
        .expect("the holder commits");
    timeout(PROMPT, deleter)
        .await
        .expect("the delete ends once no share is held")
        .expect("the deleter ran")
        .expect("the delete commits");
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn a_waiting_delete_goes_before_later_exclusive_lockers() {
    let (schema, id) = with_row().await;
    let pool = schema.pool().clone();
    let (held, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let holder = tokio::spawn({
        let pool = pool.clone();
        let (held, release) = (Arc::clone(&held), Arc::clone(&release));
        async move {
            pool.transaction(IsolationLevel::ReadCommitted, async move |tx| {
                tx.find_for_update::<ProvedRow>(&id).await?;
                held.notify_one();
                release.notified().await;
                Ok::<_, DatabaseError>(())
            })
            .await
        }
    });
    held.notified().await;
    let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let deleter = tokio::spawn({
        let order = Arc::clone(&order);
        let deleting = delete(pool.clone(), id);
        async move {
            let deleted = deleting.await;
            if let Ok(mut order) = order.lock() {
                order.push("delete");
            }
            deleted
        }
    });
    sleep(SETTLE).await;
    let mut later = Vec::new();
    for _ in 0..3 {
        later.push(tokio::spawn({
            let pool = pool.clone();
            let order = Arc::clone(&order);
            async move {
                let found = pool
                    .transaction(IsolationLevel::ReadCommitted, async |tx| {
                        tx.find_for_update::<ProvedRow>(&id).await
                    })
                    .await;
                if let Ok(mut order) = order.lock() {
                    order.push(if matches!(found, Ok(Some(_))) {
                        "found"
                    } else {
                        "gone"
                    });
                }
            }
        }));
        sleep(Duration::from_millis(200)).await;
    }
    assert!(
        !deleter.is_finished(),
        "the delete did not wait on the lock"
    );

    release.notify_one();
    holder
        .await
        .expect("the holder ran")
        .expect("the holder commits");
    let deleted = timeout(PROMPT, deleter)
        .await
        .expect("the delete ends")
        .expect("the deleter ran")
        .expect("the delete was spawned")
        .expect("the delete commits");
    assert_eq!(deleted, 1);
    for locker in later {
        timeout(PROMPT, locker)
            .await
            .expect("each later locker ends")
            .expect("the locker ran");
    }
    let order = order.lock().map(|order| order.clone()).unwrap_or_default();
    assert_eq!(order, ["delete", "gone", "gone", "gone"]);
}
