//! A bounded delete removes only rows that still match its filter when it reaches them; requires Docker.

use std::time::Duration;

use fundamental_db::{
    Filter, IsolationLevel, OrderBy, Repository, RepositoryTransactionQuery, Transaction,
    Transactional,
};
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;
use sqlx::Row as _;

#[derive(Clone, Debug, Model)]
#[model(sql)]
struct Holding {
    #[model(id)]
    holding_id: i64,
    expires: i64,
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_bounded_delete_spares_a_row_changed_after_it_was_chosen() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS holdings (holding_id BIGINT, expires BIGINT NOT NULL, \
         PRIMARY KEY (holding_id HASH))",
    )
    .await
    .expect("the table");
    pool.execute("INSERT INTO holdings (holding_id, expires) VALUES (1, 10), (2, 10)")
        .await
        .expect("the rows");

    let mut extension = pool
        .begin(IsolationLevel::ReadCommitted)
        .await
        .expect("a transaction");
    extension
        .execute("UPDATE holdings SET expires = 99 WHERE holding_id = 1")
        .await
        .expect("extended, not committed");

    let expired = pool
        .repository::<Holding>()
        .query()
        .filter(Filter::lt(Holding::EXPIRES, 50_i64))
        .order_by(OrderBy::asc(Holding::HOLDING_ID))
        .limit(10);
    let sweep = tokio::spawn({
        let pool = pool.clone();
        async move {
            pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
                tx.delete_where(&expired).await
            })
            .await
        }
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    extension.commit().await.expect("the extension commits");
    let deleted = sweep
        .await
        .expect("the sweep ran")
        .expect("the sweep committed");

    let left: Vec<i64> = pool
        .fetch_all("SELECT holding_id FROM holdings ORDER BY holding_id")
        .await
        .expect("what is left")
        .iter()
        .map(|row| row.try_get(0).expect("an id"))
        .collect();
    assert_eq!(deleted, 1, "only the row still expired is deleted");
    assert_eq!(left, [1], "the extended holding survives the sweep");
}
