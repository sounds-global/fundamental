//! `SqlTransaction::insert_or_lock`; requires Docker.

use std::sync::Arc;
use std::time::Duration;

use fundamental_db::{
    DatabaseError, IsolationLevel, KeyedRepositoryTransaction, RepositoryTransaction as _,
};
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct HeldRow {
    #[model(id)]
    id: Uuid,
    label: String,
}

async fn with_table() -> SqlSchema {
    let schema = SqlSchema::mint().await.expect("a test schema");
    schema
        .pool()
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS held_rows (id UUID NOT NULL, label TEXT NOT NULL, \
             PRIMARY KEY (id HASH))",
        )
        .await
        .expect("the table exists");
    schema
}

fn row(id: Uuid, label: &str) -> HeldRow {
    HeldRow {
        id,
        label: label.to_owned(),
    }
}

#[tokio::test]
async fn an_absent_row_is_inserted_and_returned() {
    let schema = with_table().await;
    let id = Uuid::new_v4();
    let held = schema
        .pool()
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.insert_or_lock(&row(id, "new")).await
        })
        .await
        .expect("the row is written");
    assert_eq!(held, row(id, "new"));
}

#[tokio::test]
async fn a_stored_row_keeps_every_stored_value() {
    let schema = with_table().await;
    let id = Uuid::new_v4();
    schema
        .pool()
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.insert(&row(id, "stored")).await
        })
        .await
        .expect("the row is written");
    let held = schema
        .pool()
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.insert_or_lock(&row(id, "proposed")).await
        })
        .await
        .expect("the row is locked");
    assert_eq!(held, row(id, "stored"));
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_delete_committed_while_it_waits_leaves_its_own_row() {
    let schema = with_table().await;
    let id = Uuid::new_v4();
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&row(id, "stored")).await
        })
        .await
        .expect("the row is written");

    let deleting = Arc::new(Notify::new());
    let deleter = {
        let pool = schema.pool().clone();
        let deleting = Arc::clone(&deleting);
        tokio::spawn(async move {
            pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
                tx.find_for_update::<HeldRow>(&id).await?;
                KeyedRepositoryTransaction::<HeldRow, _>::delete(tx, &id).await?;
                deleting.notify_one();
                tokio::time::sleep(Duration::from_millis(300)).await;
                Ok::<_, DatabaseError>(())
            })
            .await
        })
    };
    deleting.notified().await;
    let held = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert_or_lock(&row(id, "after")).await
        })
        .await
        .expect("the row is ensured after the delete");
    deleter
        .await
        .expect("the deleter ran")
        .expect("the delete commits");
    assert_eq!(held, row(id, "after"));
}
