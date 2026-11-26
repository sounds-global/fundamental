//! `SqlTransaction::find_many_for_update` and `SqlTransaction::isolation`; requires Docker.

use fundamental_db::{DatabaseError, IsolationLevel, RepositoryTransaction as _};
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct LockedRow {
    #[model(id)]
    id: Uuid,
    label: String,
}

async fn with_rows(labels: &[&str]) -> (SqlSchema, Vec<LockedRow>) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    schema
        .pool()
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS locked_rows (id UUID NOT NULL, label TEXT NOT NULL, \
             PRIMARY KEY (id HASH))",
        )
        .await
        .expect("the table exists");
    let rows: Vec<LockedRow> = labels
        .iter()
        .map(|label| LockedRow {
            id: Uuid::new_v4(),
            label: (*label).to_owned(),
        })
        .collect();
    let written = rows.clone();
    schema
        .pool()
        .transaction(IsolationLevel::Serializable, async |tx| {
            for row in &written {
                tx.insert(row).await?;
            }
            Ok::<_, DatabaseError>(())
        })
        .await
        .expect("the rows are written");
    (schema, rows)
}

#[tokio::test]
async fn the_rows_that_exist_come_back_once_and_in_key_order() {
    let (schema, rows) = with_rows(&["a", "b", "c"]).await;
    let mut asked: Vec<Uuid> = rows.iter().map(|row| row.id).rev().collect();
    asked.push(rows[0].id);
    asked.push(Uuid::new_v4());

    let locked = schema
        .pool()
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.find_many_for_update::<LockedRow>(&asked).await
        })
        .await
        .expect("the lock is taken");

    let mut expected = rows;
    expected.sort_by_key(|row| row.id);
    assert_eq!(locked, expected);
}

#[tokio::test]
async fn more_keys_than_the_bound_are_refused() {
    let (schema, _rows) = with_rows(&[]).await;
    let asked: Vec<Uuid> = (0..1_001).map(|_| Uuid::new_v4()).collect();

    let refused = schema
        .pool()
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.find_many_for_update::<LockedRow>(&asked).await
        })
        .await
        .expect_err("1 001 keys exceed the bound");
    assert!(
        refused.to_string().contains("the bound is 1000"),
        "refused as {refused}"
    );
}

#[tokio::test]
async fn a_transaction_reports_the_isolation_it_was_begun_at() {
    let (schema, _rows) = with_rows(&[]).await;
    for level in [IsolationLevel::Serializable, IsolationLevel::RepeatableRead] {
        let reported = schema
            .pool()
            .transaction(level, async |tx| Ok::<_, DatabaseError>(tx.isolation()))
            .await
            .expect("the transaction commits");
        assert_eq!(reported, Some(level));
    }
}
