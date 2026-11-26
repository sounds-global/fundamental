//! A delete with an order and a limit removes at most that many rows, in that order; requires Docker.

use fundamental_db::{Filter, IsolationLevel, OrderBy, Repository, RepositoryTransactionQuery};
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;
use sqlx::Row as _;

#[derive(Clone, Debug, Model)]
#[model(sql)]
struct Expiry {
    #[model(id)]
    expiry_id: i64,
    bucket: i32,
    aged: i64,
}

#[tokio::test]
async fn a_limited_delete_deletes_at_most_that_many_oldest_first() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS expiries (expiry_id BIGINT, bucket INT NOT NULL, aged BIGINT NOT NULL, \
         PRIMARY KEY (expiry_id HASH))",
    )
    .await
    .expect("the table");
    pool.execute(
        "INSERT INTO expiries (expiry_id, bucket, aged) \
         SELECT n, (n % 2)::INT, n FROM generate_series(1, 250) AS n",
    )
    .await
    .expect("the rows");

    let batch = pool
        .repository::<Expiry>()
        .query()
        .filter(Filter::eq(Expiry::BUCKET, 0_i32))
        .order_by(OrderBy::asc(Expiry::AGED))
        .limit(50);
    let first = pool
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.delete_where(&batch).await
        })
        .await
        .expect("the first batch");
    assert_eq!(first, 50, "the limit was not honoured");

    let row = pool
        .fetch_one("SELECT count(*), min(aged) FROM expiries WHERE bucket = 0")
        .await
        .expect("what is left");
    let left: i64 = row.try_get(0).expect("a count");
    let oldest: i64 = row.try_get(1).expect("a minimum");
    assert_eq!(left, 75);
    assert_eq!(oldest, 102, "the batch was not the oldest fifty");
    let other: i64 = pool
        .fetch_one("SELECT count(*) FROM expiries WHERE bucket = 1")
        .await
        .expect("the other bucket")
        .try_get(0)
        .expect("a count");
    assert_eq!(other, 125, "the filter was not honoured");
}
