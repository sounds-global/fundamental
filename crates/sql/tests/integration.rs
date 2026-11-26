//! Integration tests for the YugabyteDB YSQL adapter; these require Docker.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use std::collections::{BTreeMap, HashMap};

use fundamental_core::Error;
use fundamental_core::model::SqlDb;
use fundamental_core::secrets::{Secret, SecretBytes, SecretJson, SecretString};
use fundamental_db::migration::{
    DatabaseSchema, MigrationDatabase, ModelSchema, SchemaDiffer, SchemaIntrospector,
};
use fundamental_db::{
    ConnectionPool, ConstraintKind, DatabaseError, Filter, FilterValue, IsolationLevel,
    KeyedRepository, KeyedRepositoryBatch, KeyedRepositoryTransaction,
    KeyedRepositoryTransactionBatch, QueryBuilder, QueryExecutor, Repository, RepositoryAggregate,
    RepositoryBatch, RepositoryExt, RepositorySoftDelete, RepositoryTransaction,
    RepositoryTransactionAggregate, RepositoryTransactionBatch, RepositoryTransactionExt,
    RepositoryTransactionQuery, RepositoryTransactionSoftDelete, RepositoryTransactionTranslation,
    Transaction, TransactionScope, Transactional,
};
use fundamental_macros::Model;
use fundamental_sql::migration::SqlIntrospector;
use fundamental_sql::{
    BatchConfig, CircuitBreaker, CircuitBreakerConfig, CircuitState, ColumnGuard,
    PrometheusMetrics, SequenceRef, SqlConfig, SqlConnections, SqlError, SqlMetrics,
    SqlMetricsConfig, SqlModelQuery, SqlPool, SqlResult, SqlSliceStore, WriteOutcome,
};
use fundamental_sync::{SyncMeta, SyncRecord};

use fundamental_test_containers::sql::SqlSchema;

/// The two tables these tests share, over a [`SqlSchema`] that owns the node and teardown.
struct SqlTestFixture {
    pool: SqlPool,
    _schema: SqlSchema,
}

impl SqlTestFixture {
    async fn new() -> Self {
        let schema = SqlSchema::mint().await.expect("a test schema");
        Self {
            pool: schema.pool().clone(),
            _schema: schema,
        }
    }

    async fn create_users_table(&self) {
        let sql = r#"
            CREATE TABLE IF NOT EXISTS users (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                email TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                created_at TIMESTAMPTZ DEFAULT now()
            )
        "#;

        self.pool
            .execute_ddl(sql)
            .await
            .expect("Failed to create users table");
    }

    async fn create_orders_table(&self) {
        let sql = r#"
            CREATE TABLE IF NOT EXISTS orders (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                user_id UUID NOT NULL REFERENCES users(id),
                total DECIMAL(10, 2) NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TIMESTAMPTZ DEFAULT now()
            )
        "#;

        self.pool
            .execute_ddl(sql)
            .await
            .expect("Failed to create orders table");
    }
}

#[tokio::test]
async fn test_connection_and_health_check() {
    let fixture = SqlTestFixture::new().await;

    assert!(fixture.pool.health_check().await.is_ok());
}

#[tokio::test]
async fn test_pool_status() {
    let fixture = SqlTestFixture::new().await;

    let status = fixture.pool.status();
    let min = status
        .min
        .expect("the SQL wire reports its configured floor");
    let active = status
        .active
        .expect("the SQL wire counts connections in use");
    let idle = status.idle.expect("the SQL wire counts idle connections");
    assert!(status.max >= min);
    assert!(active + idle <= status.max);
    assert_eq!(
        status.pending,
        Some(0),
        "an idle pool has nobody waiting, and reports that as a measured zero"
    );
}

#[tokio::test]
async fn test_insert_and_select() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let user_id = uuid::Uuid::new_v4();
    let email = "test@example.com";
    let name = "Test User";

    fixture
        .pool
        .execute(&format!(
            "INSERT INTO users (id, email, name) VALUES ('{}', '{}', '{}')",
            user_id, email, name
        ))
        .await
        .expect("Failed to insert user");

    let rows = fixture
        .pool
        .fetch_all(&format!(
            "SELECT email, name FROM users WHERE id = '{}'",
            user_id
        ))
        .await
        .expect("Failed to select user");

    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    let retrieved_email: &str = sqlx::Row::get(row, "email");
    let retrieved_name: &str = sqlx::Row::get(row, "name");

    assert_eq!(retrieved_email, email);
    assert_eq!(retrieved_name, name);
}

#[tokio::test]
async fn test_transaction_commit() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let user_id = uuid::Uuid::new_v4();
    let email = "tx_test@example.com";

    let mut tx = fixture
        .pool
        .begin(IsolationLevel::Serializable)
        .await
        .expect("Failed to begin transaction");

    tx.execute(&format!(
        "INSERT INTO users (id, email, name) VALUES ('{}', '{}', 'TX Test')",
        user_id, email
    ))
    .await
    .expect("Failed to insert in transaction");

    tx.commit().await.expect("Failed to commit transaction");

    let rows = fixture
        .pool
        .fetch_all(&format!("SELECT email FROM users WHERE id = '{}'", user_id))
        .await
        .expect("Failed to select user");

    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn test_transaction_rollback() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let user_id = uuid::Uuid::new_v4();
    let email = "rollback_test@example.com";

    let mut tx = fixture
        .pool
        .begin(IsolationLevel::Serializable)
        .await
        .expect("Failed to begin transaction");

    tx.execute(&format!(
        "INSERT INTO users (id, email, name) VALUES ('{}', '{}', 'Rollback Test')",
        user_id, email
    ))
    .await
    .expect("Failed to insert in transaction");

    tx.rollback().await.expect("Failed to rollback transaction");

    let rows = fixture
        .pool
        .fetch_all(&format!("SELECT email FROM users WHERE id = '{}'", user_id))
        .await
        .expect("Failed to select user");

    assert_eq!(rows.len(), 0);
}

#[tokio::test]
async fn test_savepoint() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let user1_id = uuid::Uuid::new_v4();
    let user2_id = uuid::Uuid::new_v4();

    let mut tx = fixture
        .pool
        .begin(IsolationLevel::Serializable)
        .await
        .expect("Failed to begin transaction");

    tx.execute(&format!(
        "INSERT INTO users (id, email, name) VALUES ('{}', 'user1@example.com', 'User 1')",
        user1_id
    ))
    .await
    .expect("Failed to insert user 1");

    tx.execute_savepoint("sp1")
        .await
        .expect("Failed to create savepoint");

    tx.execute(&format!(
        "INSERT INTO users (id, email, name) VALUES ('{}', 'user2@example.com', 'User 2')",
        user2_id
    ))
    .await
    .expect("Failed to insert user 2");

    tx.execute_rollback_to_savepoint("sp1")
        .await
        .expect("Failed to rollback to savepoint");

    tx.commit().await.expect("Failed to commit transaction");

    let rows = fixture
        .pool
        .fetch_all("SELECT id FROM users")
        .await
        .expect("Failed to select users");

    assert_eq!(rows.len(), 1);
    let id: uuid::Uuid = sqlx::Row::get(&rows[0], "id");
    assert_eq!(id, user1_id);
}

#[tokio::test]
async fn test_savepoint_via_trait() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let user1_id = uuid::Uuid::new_v4();
    let user2_id = uuid::Uuid::new_v4();

    let mut tx = fixture
        .pool
        .begin(IsolationLevel::Serializable)
        .await
        .expect("Failed to begin transaction");

    tx.execute(&format!(
        "INSERT INTO users (id, email, name) VALUES ('{}', 'sp_user1@example.com', 'User 1')",
        user1_id
    ))
    .await
    .expect("Failed to insert user 1");

    TransactionScope::savepoint(&mut tx, "sp1")
        .await
        .expect("trait savepoint should succeed");

    tx.execute(&format!(
        "INSERT INTO users (id, email, name) VALUES ('{}', 'sp_user2@example.com', 'User 2')",
        user2_id
    ))
    .await
    .expect("Failed to insert user 2");

    TransactionScope::rollback_to_savepoint(&mut tx, "sp1")
        .await
        .expect("trait rollback_to_savepoint should succeed");

    TransactionScope::release_savepoint(&mut tx, "sp1")
        .await
        .expect("trait release_savepoint should succeed");

    tx.commit().await.expect("Failed to commit transaction");

    let rows = fixture
        .pool
        .fetch_all("SELECT id FROM users")
        .await
        .expect("Failed to select users");

    assert_eq!(rows.len(), 1);
    let id: uuid::Uuid = sqlx::Row::get(&rows[0], "id");
    assert_eq!(id, user1_id);
}

#[tokio::test]
async fn test_savepoint_via_trait_rejects_invalid_name() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let mut tx = fixture
        .pool
        .begin(IsolationLevel::Serializable)
        .await
        .expect("Failed to begin transaction");

    let result = TransactionScope::savepoint(&mut tx, "not-valid").await;
    let err = result.expect_err("invalid savepoint name must be rejected");
    assert_eq!(err.error_code(), "SQL_006");
}

#[tokio::test]
async fn test_transaction_helper_commits() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let user_id = uuid::Uuid::new_v4();

    let outcome: Result<(), DatabaseError> = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.execute(&format!(
                "INSERT INTO users (id, email, name) VALUES ('{}', 'helper_ok@example.com', 'Helper')",
                user_id
            ))
            .await?;
            Ok(())
        })
        .await;

    outcome.expect("helper should commit on Ok");

    let rows = fixture
        .pool
        .fetch_all(&format!("SELECT email FROM users WHERE id = '{}'", user_id))
        .await
        .expect("Failed to select user");
    assert_eq!(rows.len(), 1, "row must persist after helper commit");
}

#[tokio::test]
async fn test_transaction_helper_rolls_back_on_err() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let user_id = uuid::Uuid::new_v4();

    let outcome: Result<(), DatabaseError> = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.execute(&format!(
                "INSERT INTO users (id, email, name) VALUES ('{}', 'helper_err@example.com', 'Helper')",
                user_id
            ))
            .await?;
            Err(DatabaseError::query("boom"))
        })
        .await;

    let err = outcome.expect_err("closure error must propagate");
    assert!(matches!(err, DatabaseError::Query { .. }));
    assert_eq!(err.error_code(), "DB_002");

    let rows = fixture
        .pool
        .fetch_all(&format!("SELECT email FROM users WHERE id = '{}'", user_id))
        .await
        .expect("Failed to select user");
    assert_eq!(rows.len(), 0, "row must be rolled back after helper error");
}

#[tokio::test]
async fn test_transaction_helper_with_isolation() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let user_id = uuid::Uuid::new_v4();

    let outcome: Result<(), DatabaseError> = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.execute(&format!(
                "INSERT INTO users (id, email, name) VALUES ('{}', 'helper_iso@example.com', 'Helper')",
                user_id
            ))
            .await?;
            Ok(())
        })
        .await;

    outcome.expect("helper should commit under Serializable");

    let rows = fixture
        .pool
        .fetch_all(&format!("SELECT email FROM users WHERE id = '{}'", user_id))
        .await
        .expect("Failed to select user");
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn test_unique_constraint_violation() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let email = "duplicate@example.com";

    fixture
        .pool
        .execute(&format!(
            "INSERT INTO users (id, email, name) VALUES ('{}', '{}', 'User 1')",
            uuid::Uuid::new_v4(),
            email
        ))
        .await
        .expect("Failed to insert first user");

    let result = fixture
        .pool
        .execute(&format!(
            "INSERT INTO users (id, email, name) VALUES ('{}', '{}', 'User 2')",
            uuid::Uuid::new_v4(),
            email
        ))
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("duplicate") || err.to_string().contains("unique"));
}

#[tokio::test]
async fn test_foreign_key_constraint() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;
    fixture.create_orders_table().await;

    let result = fixture
        .pool
        .execute(&format!(
            "INSERT INTO orders (id, user_id, total, status) VALUES ('{}', '{}', 100.00, 'pending')",
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4() // Non-existent user
        ))
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("foreign key") || err.to_string().contains("violates"));
}

#[tokio::test]
async fn test_batch_insert() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let mut sql = String::from("INSERT INTO users (id, email, name) VALUES ");
    let mut values = Vec::new();

    for i in 0..10 {
        let id = uuid::Uuid::new_v4();
        let email = format!("batch{}@example.com", i);
        values.push(format!("('{}', '{}', 'Batch User {}')", id, email, i));
    }

    sql.push_str(&values.join(", "));

    fixture
        .pool
        .execute(&sql)
        .await
        .expect("Failed to batch insert users");

    let row = fixture
        .pool
        .fetch_one("SELECT COUNT(*) as count FROM users")
        .await
        .expect("Failed to count users");

    let count: i64 = sqlx::Row::get(&row, "count");
    assert_eq!(count, 10);
}

#[tokio::test]
async fn test_metrics_collection() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let metrics = Arc::new(SqlMetrics::new(
        SqlMetricsConfig::new().slow_query_threshold(Duration::from_millis(1)),
    ));

    let user_id = uuid::Uuid::new_v4();
    let email = "metrics@example.com";

    let metrics_clone = Arc::clone(&metrics);
    metrics_clone
        .timed("insert", "users", async {
            fixture
                .pool
                .execute(&format!(
                    "INSERT INTO users (id, email, name) VALUES ('{}', '{}', 'Metrics Test')",
                    user_id, email
                ))
                .await
                .map_err(|e| e.to_string())
        })
        .await
        .expect("Failed to insert with timing");

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.base.queries_total, 1);
}

#[tokio::test]
async fn test_transaction_metrics() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let metrics = Arc::new(SqlMetrics::with_defaults());

    metrics.record_transaction_start();
    let mut tx = fixture
        .pool
        .begin(IsolationLevel::Serializable)
        .await
        .expect("Failed to begin transaction");

    tx.execute(&format!(
        "INSERT INTO users (id, email, name) VALUES ('{}', 'tx1@example.com', 'TX 1')",
        uuid::Uuid::new_v4()
    ))
    .await
    .expect("Failed to insert");

    tx.commit().await.expect("Failed to commit");
    metrics.record_transaction_commit(Duration::from_millis(50));

    metrics.record_transaction_start();
    let mut tx = fixture
        .pool
        .begin(IsolationLevel::Serializable)
        .await
        .expect("Failed to begin transaction");

    tx.execute(&format!(
        "INSERT INTO users (id, email, name) VALUES ('{}', 'tx2@example.com', 'TX 2')",
        uuid::Uuid::new_v4()
    ))
    .await
    .expect("Failed to insert");

    tx.rollback().await.expect("Failed to rollback");
    metrics.record_transaction_rollback(Duration::from_millis(30));

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.base.transactions_total, 2);
    assert_eq!(snapshot.base.transactions_committed, 1);
    assert_eq!(snapshot.transactions_rolled_back, 1);
}

#[tokio::test]
async fn test_serialization_conflict_handling() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let user_id = uuid::Uuid::new_v4();
    fixture
        .pool
        .execute(&format!(
            "INSERT INTO users (id, email, name) VALUES ('{}', 'conflict@example.com', 'Initial')",
            user_id
        ))
        .await
        .expect("Failed to insert initial user");

    let result = fixture
        .pool
        .with_retry(|| async {
            let mut tx = fixture
                .pool
                .begin(IsolationLevel::Serializable)
                .await
                .map_err(|e| SqlError::Query(e.to_string()))?;

            tx.fetch_one(&format!("SELECT name FROM users WHERE id = '{}'", user_id))
                .await?;

            tx.execute(&format!(
                "UPDATE users SET name = 'Updated' WHERE id = '{}'",
                user_id
            ))
            .await?;

            tx.commit()
                .await
                .map_err(|e| SqlError::Query(e.to_string()))?;
            Ok(())
        })
        .await;

    assert!(result.is_ok());

    let row = fixture
        .pool
        .fetch_one(&format!("SELECT name FROM users WHERE id = '{}'", user_id))
        .await
        .expect("Failed to select user");

    let name: &str = sqlx::Row::get(&row, "name");
    assert_eq!(name, "Updated");
}

#[tokio::test]
async fn test_isolation_level() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;

    let mut tx = fixture
        .pool
        .begin(IsolationLevel::Serializable)
        .await
        .expect("Failed to begin transaction with isolation level");

    tx.execute(&format!(
        "INSERT INTO users (id, email, name) VALUES ('{}', 'isolation@example.com', 'Isolation Test')",
        uuid::Uuid::new_v4()
    ))
    .await
    .expect("Failed to insert in isolated transaction");

    tx.commit().await.expect("Failed to commit");
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct TestUser {
    #[model(id)]
    id: uuid::Uuid,
    email: String,
    name: String,
}

impl SqlTestFixture {
    async fn create_test_users(&self) {
        self.pool
            .execute_ddl(
                "CREATE TABLE IF NOT EXISTS test_users (\
                    id UUID PRIMARY KEY, email TEXT UNIQUE NOT NULL, name TEXT NOT NULL)",
            )
            .await
            .expect("Failed to create test_users table");
    }
}

#[tokio::test]
async fn test_repository_crud_round_trip() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;
    let repo = fixture.pool.repository::<TestUser>();

    let user = TestUser {
        id: uuid::Uuid::new_v4(),
        email: "repo@example.com".into(),
        name: "Repo User".into(),
    };

    let inserted = repo.insert(&user).await.expect("insert");
    assert_eq!(inserted, user);

    assert!(repo.exists(&user.id).await.expect("exists"));
    let found = repo.find(&user.id).await.expect("find").expect("present");
    assert_eq!(found, user);

    let updated = TestUser {
        name: "Renamed".into(),
        ..user.clone()
    };
    let returned = repo.update(&updated).await.expect("update");
    assert_eq!(returned.name, "Renamed");
    assert_eq!(repo.find(&user.id).await.unwrap().unwrap().name, "Renamed");

    let upserted = TestUser {
        name: "Upserted".into(),
        ..user.clone()
    };
    repo.upsert(&upserted).await.expect("upsert");
    assert_eq!(repo.find(&user.id).await.unwrap().unwrap().name, "Upserted");

    assert_eq!(RepositoryAggregate::count(&repo).await.expect("count"), 1);
    let all = repo.find_all(10, 0).await.expect("find_all");
    assert_eq!(all.len(), 1);

    assert!(repo.delete(&user.id).await.expect("delete"));
    assert!(!repo.exists(&user.id).await.expect("exists after delete"));
}

#[tokio::test]
async fn test_insert_or_ignore_is_idempotent() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;
    let repo = fixture.pool.repository::<TestUser>();

    let user = TestUser {
        id: uuid::Uuid::new_v4(),
        email: "ignore@example.com".into(),
        name: "Original".into(),
    };

    let inserted = repo
        .insert_or_ignore(&user, &["id"])
        .await
        .expect("first insert_or_ignore");
    assert_eq!(inserted, Some(user.clone()));

    let colliding = TestUser {
        id: uuid::Uuid::new_v4(),
        email: user.email.clone(),
        name: "Should Not Overwrite".into(),
    };
    let ignored = repo
        .insert_or_ignore(&colliding, &["email"])
        .await
        .expect("conflicting insert_or_ignore");
    assert_eq!(ignored, None);
    assert_eq!(
        repo.find(&user.id).await.unwrap().unwrap().name,
        "Original",
        "insert_or_ignore must not overwrite the existing row"
    );
    assert!(
        !repo.exists(&colliding.id).await.unwrap(),
        "the ignored row's id must never have landed"
    );

    let err = repo
        .insert_or_ignore(&user, &["email); DROP TABLE test_users --"])
        .await
        .expect_err("an invalid identifier must be rejected");
    assert!(matches!(err, SqlError::Query(_)));
}

#[tokio::test]
async fn test_insert_if_absent_inside_a_transaction_reports_whether_it_wrote() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;
    let user = TestUser {
        id: uuid::Uuid::new_v4(),
        email: "absent@example.com".into(),
        name: "Original".into(),
    };
    let same_key = TestUser {
        name: "Should Not Overwrite".into(),
        ..user.clone()
    };

    let (first, second) = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            let first = tx.insert_if_absent(&user).await?;
            let second = tx.insert_if_absent(&same_key).await?;
            Ok::<_, DatabaseError>((first, second))
        })
        .await
        .expect("the transaction commits");
    assert_eq!(first, Some(user.clone()));
    assert_eq!(second, None, "a present key is a no-op, not an error");

    let redelivered = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.insert_if_absent(&same_key).await
        })
        .await
        .expect("a redelivery commits");
    assert_eq!(
        redelivered, None,
        "a later transaction sees the committed key"
    );

    let repo = fixture.pool.repository::<TestUser>();
    assert_eq!(repo.find(&user.id).await.unwrap().unwrap().name, "Original");
    assert_eq!(RepositoryAggregate::count(&repo).await.unwrap(), 1);
}

#[tokio::test]
async fn test_insert_or_ignore_inside_a_transaction_honours_a_unique_constraint() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;
    let user = TestUser {
        id: uuid::Uuid::new_v4(),
        email: "unique@example.com".into(),
        name: "Original".into(),
    };
    let same_email = TestUser {
        id: uuid::Uuid::new_v4(),
        email: user.email.clone(),
        name: "Should Not Land".into(),
    };

    let (first, ignored) = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            let refused = tx
                .insert_or_ignore(&user, &["email); DROP TABLE test_users --"])
                .await;
            assert!(matches!(refused, Err(SqlError::Query(_))));
            let first = tx
                .insert_or_ignore(&user, &["id"])
                .await
                .map_err(DatabaseError::from)?;
            let ignored = tx
                .insert_or_ignore(&same_email, &["email"])
                .await
                .map_err(DatabaseError::from)?;
            Ok::<_, DatabaseError>((first, ignored))
        })
        .await
        .expect("a refused identifier does not abort the transaction");
    assert_eq!(first, Some(user.clone()));
    assert_eq!(ignored, None);

    let repo = fixture.pool.repository::<TestUser>();
    assert!(!repo.exists(&same_email.id).await.unwrap());
    assert_eq!(RepositoryAggregate::count(&repo).await.unwrap(), 1);
}

#[tokio::test]
async fn test_insert_if_absent_conflicts_on_the_whole_composite_key() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_zoned_rows().await;
    let row_id = uuid::Uuid::new_v4();
    let eu = ZonedRow {
        zone: "eu".into(),
        row_id,
        payload: "first".into(),
    };
    let eu_again = ZonedRow {
        payload: "should not overwrite".into(),
        ..eu.clone()
    };
    let uk = ZonedRow {
        zone: "uk".into(),
        ..eu.clone()
    };

    let (first, again, other_zone) = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            let first = tx.insert_if_absent(&eu).await?;
            let again = tx.insert_if_absent(&eu_again).await?;
            let other_zone = tx.insert_if_absent(&uk).await?;
            Ok::<_, DatabaseError>((first, again, other_zone))
        })
        .await
        .expect("the transaction commits");
    assert_eq!(first, Some(eu.clone()));
    assert_eq!(again, None, "the same (zone, row_id) is present");
    assert_eq!(
        other_zone,
        Some(uk),
        "the same row_id in another zone is another row"
    );

    let repo = fixture.pool.repository::<ZonedRow>();
    let stored = fixture
        .pool
        .query_executor::<ZonedRow>()
        .execute(&repo.query().pin_key("eu"))
        .await
        .expect("reads back");
    assert_eq!(stored, vec![eu], "the existing row is left untouched");
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct GuardedHead {
    #[model(id)]
    origin: String,
    tree_size: i64,
    note: String,
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct SeqLeaf {
    #[model(id)]
    idx: i64,
    commitment: String,
    label: String,
}

impl SqlTestFixture {
    async fn create_guarded_heads(&self) {
        self.pool
            .execute_ddl(
                "CREATE TABLE IF NOT EXISTS guarded_heads (\
                    origin TEXT PRIMARY KEY, tree_size INT8 NOT NULL, note TEXT NOT NULL)",
            )
            .await
            .expect("create guarded_heads");
    }

    async fn create_seq_leaves(&self) {
        self.pool
            .execute_ddl(
                "CREATE TABLE IF NOT EXISTS seq_leaves (\
                    idx INT8 PRIMARY KEY, commitment TEXT UNIQUE NOT NULL, label TEXT NOT NULL)",
            )
            .await
            .expect("create seq_leaves");
        self.pool
            .execute_ddl(
                "CREATE TABLE IF NOT EXISTS seq_counter (id INT8 PRIMARY KEY, next_val INT8 NOT NULL)",
            )
            .await
            .expect("create seq_counter");
        self.pool
            .execute("INSERT INTO seq_counter (id, next_val) VALUES (0, 0)")
            .await
            .expect("seed seq_counter");
    }
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn test_upsert_guarded_is_a_monotonic_compare_and_set() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_guarded_heads().await;
    let repo = fixture.pool.repository::<GuardedHead>();

    let head = |size: i64, note: &str| GuardedHead {
        origin: "log-a".into(),
        tree_size: size,
        note: note.into(),
    };
    let guard = || ColumnGuard::excluded_ge("tree_size");

    let applied = repo
        .upsert_guarded(&head(5, "at-5"), &["origin"], &guard())
        .await
        .expect("insert");
    assert_eq!(applied.map(|h| h.tree_size), Some(5));

    let blocked = repo
        .upsert_guarded(&head(3, "regress"), &["origin"], &guard())
        .await
        .expect("guarded upsert");
    assert_eq!(blocked, None, "an older size must not overwrite");
    assert_eq!(
        repo.find(&"log-a".to_string())
            .await
            .unwrap()
            .unwrap()
            .tree_size,
        5
    );
    assert_eq!(
        repo.find(&"log-a".to_string()).await.unwrap().unwrap().note,
        "at-5"
    );

    let equal = repo
        .upsert_guarded(&head(5, "again-5"), &["origin"], &guard())
        .await
        .expect("guarded upsert");
    assert_eq!(equal.map(|h| h.note), Some("again-5".to_string()));

    let advanced = repo
        .upsert_guarded(&head(8, "at-8"), &["origin"], &guard())
        .await
        .expect("guarded upsert");
    assert_eq!(advanced.map(|h| h.tree_size), Some(8));
    assert_eq!(
        repo.find(&"log-a".to_string())
            .await
            .unwrap()
            .unwrap()
            .tree_size,
        8
    );

    let strict = repo
        .upsert_guarded(
            &head(8, "strict"),
            &["origin"],
            &ColumnGuard::excluded_gt("tree_size"),
        )
        .await
        .expect("guarded upsert");
    assert_eq!(strict, None, "`>` must reject an equal size");

    let err = repo
        .upsert_guarded(
            &head(9, "x"),
            &["origin"],
            &ColumnGuard::excluded_ge("size; DROP TABLE guarded_heads --"),
        )
        .await
        .expect_err("invalid guard column must be rejected");
    assert!(matches!(err, SqlError::Query(_)));
}

#[tokio::test]
async fn test_insert_or_get_sequenced_is_dense_and_idempotent() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_seq_leaves().await;
    let repo = fixture.pool.repository::<SeqLeaf>();
    let seq = SequenceRef {
        table: "seq_counter",
        key_column: "id",
        key: 0,
        next_column: "next_val",
    };

    let leaf = |commitment: &str, label: &str| SeqLeaf {
        idx: 999,
        commitment: commitment.into(),
        label: label.into(),
    };

    let (first, outcome) = repo
        .insert_or_get_sequenced(&leaf("aa", "first"), "commitment", &seq)
        .await
        .expect("first sequenced insert");
    assert_eq!(outcome, WriteOutcome::Inserted);
    assert_eq!(first.idx, 0);

    let (again, outcome) = repo
        .insert_or_get_sequenced(&leaf("aa", "second-should-not-apply"), "commitment", &seq)
        .await
        .expect("duplicate sequenced insert");
    assert_eq!(outcome, WriteOutcome::Existing);
    assert_eq!(again.idx, 0);
    assert_eq!(
        again.label, "first",
        "the existing row must not be overwritten"
    );

    let (third, outcome) = repo
        .insert_or_get_sequenced(&leaf("bb", "third"), "commitment", &seq)
        .await
        .expect("second distinct sequenced insert");
    assert_eq!(outcome, WriteOutcome::Inserted);
    assert_eq!(
        third.idx, 1,
        "indices must stay dense with no gap after a dedupe"
    );

    let err = repo
        .insert_or_get_sequenced(&leaf("cc", "x"), "idx", &seq)
        .await
        .expect_err("the PK is not a valid dedupe column");
    assert!(matches!(err, SqlError::Query(_)));
}

#[tokio::test]
async fn test_unique_violation_is_typed() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;

    let email = "dup-typed@example.com";
    fixture
        .pool
        .execute(&format!(
            "INSERT INTO test_users (id, email, name) VALUES ('{}', '{}', 'A')",
            uuid::Uuid::new_v4(),
            email
        ))
        .await
        .expect("first insert");

    let err = fixture
        .pool
        .execute(&format!(
            "INSERT INTO test_users (id, email, name) VALUES ('{}', '{}', 'B')",
            uuid::Uuid::new_v4(),
            email
        ))
        .await
        .expect_err("duplicate must fail");

    match err {
        SqlError::ConstraintViolation {
            kind: ConstraintKind::Unique,
            ..
        } => {}
        other => panic!("expected typed Unique ConstraintViolation, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_foreign_key_violation_is_typed() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_users_table().await;
    fixture.create_orders_table().await;

    let err = fixture
        .pool
        .execute(&format!(
            "INSERT INTO orders (id, user_id, total, status) VALUES ('{}', '{}', 10.00, 'pending')",
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4()
        ))
        .await
        .expect_err("fk violation must fail");

    match err {
        SqlError::ConstraintViolation {
            kind: ConstraintKind::ForeignKey,
            ..
        } => {}
        other => panic!("expected typed ForeignKey ConstraintViolation, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_repository_batch_chunking() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;
    let repo = fixture.pool.repository::<TestUser>();

    let users: Vec<TestUser> = (0..1001)
        .map(|i| TestUser {
            id: uuid::Uuid::new_v4(),
            email: format!("batch{i}@example.com"),
            name: format!("Batch {i}"),
        })
        .collect();

    let inserted = repo.insert_many(&users).await.expect("insert_many");
    assert_eq!(inserted.len(), 1001);
    assert_eq!(
        RepositoryAggregate::count(&repo).await.expect("count"),
        1001
    );

    let renamed: Vec<TestUser> = users
        .iter()
        .map(|u| TestUser {
            name: "Renamed".into(),
            ..u.clone()
        })
        .collect();
    repo.update_many(&renamed).await.expect("update_many");
    let some = repo.find(&users[0].id).await.unwrap().unwrap();
    assert_eq!(some.name, "Renamed");

    let upserts: Vec<TestUser> = users
        .iter()
        .map(|u| TestUser {
            name: "Upserted".into(),
            ..u.clone()
        })
        .collect();
    repo.upsert_many(&upserts).await.expect("upsert_many");
    assert_eq!(RepositoryAggregate::count(&repo).await.unwrap(), 1001);

    let ids: Vec<uuid::Uuid> = users.iter().take(10).map(|u| u.id).collect();
    assert_eq!(repo.find_many(&ids).await.expect("find_many").len(), 10);
    assert_eq!(repo.delete_many(&ids).await.expect("delete_many"), 10);
    assert_eq!(RepositoryAggregate::count(&repo).await.unwrap(), 991);

    let cfg = BatchConfig::new().insert_chunk_size(7);
    assert_eq!(cfg.insert_chunk_size, 7);
}

async fn seed_users(repo: &fundamental_sql::SqlRepository<TestUser>, names: &[&str]) {
    for (i, name) in names.iter().enumerate() {
        repo.insert(&TestUser {
            id: uuid::Uuid::new_v4(),
            email: format!("{name}-{i}@example.com"),
            name: (*name).to_string(),
        })
        .await
        .expect("seed insert");
    }
}

#[tokio::test]
async fn test_query_builder_and_executor() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;
    let repo = fixture.pool.repository::<TestUser>();
    seed_users(&repo, &["Alice", "Alice", "Bob", "Carol"]).await;

    let executor = fixture.pool.query_executor::<TestUser>();

    let alices = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("name", "Alice"));
    let found = executor.execute(&alices).await.expect("execute");
    assert_eq!(found.len(), 2);

    assert_eq!(executor.count(&alices).await.expect("count"), 2);

    let one = QueryBuilder::<TestUser, SqlDb>::new()
        .filter(Filter::eq("name", "Bob"))
        .limit(1);
    let bob = executor.execute_one(&one).await.expect("execute_one");
    assert_eq!(bob.unwrap().name, "Bob");

    let updated = executor
        .update_where(
            &alices,
            &[("name".to_string(), FilterValue::from("Alicia"))],
        )
        .await
        .expect("update_where");
    assert_eq!(updated, 2);
    assert_eq!(executor.count(&alices).await.unwrap(), 0);

    let carol = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("name", "Carol"));
    assert_eq!(
        executor.delete_where(&carol).await.expect("delete_where"),
        1
    );
    assert_eq!(RepositoryAggregate::count(&repo).await.unwrap(), 3);
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn test_with_retry_retries_serialization_conflict() {
    let fixture = SqlTestFixture::new().await;
    let attempts = Arc::new(AtomicU32::new(0));

    let result: SqlResult<i32> = fixture
        .pool
        .with_retry(|| {
            let attempts = Arc::clone(&attempts);
            async move {
                let n = attempts.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Err(SqlError::serialization_conflict("forced conflict"))
                } else {
                    Ok(42)
                }
            }
        })
        .await;

    assert_eq!(result.expect("retry should succeed"), 42);
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2,
        "should retry exactly once"
    );
}

#[tokio::test]
async fn test_with_retry_does_not_retry_other_errors() {
    let fixture = SqlTestFixture::new().await;
    let attempts = Arc::new(AtomicU32::new(0));

    let result: SqlResult<()> = fixture
        .pool
        .with_retry(|| {
            let attempts = Arc::clone(&attempts);
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(SqlError::Query("non-retryable".into()))
            }
        })
        .await;

    assert!(result.is_err());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "non-retryable must not retry"
    );
}

#[tokio::test]
async fn test_concurrent_serializable_conflict_resolves() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;

    let id = uuid::Uuid::new_v4();
    fixture
        .pool
        .repository::<TestUser>()
        .insert(&TestUser {
            id,
            email: "ctr@example.com".into(),
            name: "n0".into(),
        })
        .await
        .expect("seed");

    // with_retry absorbs any 40001 conflict, so both increments land.
    let run = |pool: SqlPool| async move {
        pool.with_retry(|| async {
            let mut tx = pool
                .begin(IsolationLevel::Serializable)
                .await
                .map_err(|e| SqlError::Query(e.to_string()))?;
            let row = tx
                .fetch_one(&format!("SELECT name FROM test_users WHERE id = '{id}'"))
                .await?;
            let cur: String = sqlx::Row::get(&row, "name");
            let next = format!("{cur}+");
            tx.execute(&format!(
                "UPDATE test_users SET name = '{next}' WHERE id = '{id}'"
            ))
            .await?;
            tx.commit()
                .await
                .map_err(|e| SqlError::Query(e.to_string()))?;
            Ok::<(), SqlError>(())
        })
        .await
    };

    let (a, b) = tokio::join!(run(fixture.pool.clone()), run(fixture.pool.clone()));
    a.expect("tx A");
    b.expect("tx B");

    let row = fixture
        .pool
        .fetch_one(&format!("SELECT name FROM test_users WHERE id = '{id}'"))
        .await
        .unwrap();
    let name: String = sqlx::Row::get(&row, "name");
    assert_eq!(name, "n0++", "both increments must be applied");
}

#[tokio::test]
async fn test_all_isolation_levels() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;

    for level in [
        IsolationLevel::ReadUncommitted,
        IsolationLevel::ReadCommitted,
        IsolationLevel::RepeatableRead,
        IsolationLevel::Serializable,
    ] {
        let mut tx = fixture
            .pool
            .begin(level)
            .await
            .unwrap_or_else(|e| panic!("begin {level:?}: {e}"));
        tx.execute(&format!(
            "INSERT INTO test_users (id, email, name) VALUES ('{}', '{:?}@x.com', '{:?}')",
            uuid::Uuid::new_v4(),
            level,
            level
        ))
        .await
        .unwrap_or_else(|e| panic!("insert under {level:?}: {e}"));
        tx.commit().await.expect("commit");
    }
}

#[tokio::test]
async fn test_savepoints_multiple_release_and_validation() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;

    let mut tx = fixture
        .pool
        .begin(IsolationLevel::Serializable)
        .await
        .expect("begin");

    let insert = |n: &str| {
        format!(
            "INSERT INTO test_users (id, email, name) VALUES ('{}', '{}@x.com', '{}')",
            uuid::Uuid::new_v4(),
            n,
            n
        )
    };

    tx.execute(&insert("a")).await.unwrap();
    tx.execute_savepoint("sp1").await.expect("sp1");
    tx.execute(&insert("b")).await.unwrap();
    tx.execute_savepoint("sp2").await.expect("sp2");
    tx.execute(&insert("c")).await.unwrap();

    tx.execute_release_savepoint("sp2")
        .await
        .expect("release sp2");
    tx.execute_rollback_to_savepoint("sp1")
        .await
        .expect("rollback to sp1");
    tx.commit().await.expect("commit");

    assert_eq!(
        RepositoryAggregate::count(&fixture.pool.repository::<TestUser>())
            .await
            .unwrap(),
        1
    );

    let mut tx = fixture
        .pool
        .begin(IsolationLevel::Serializable)
        .await
        .expect("begin2");
    assert!(tx.execute_savepoint("bad name!").await.is_err());
}

#[tokio::test]
async fn test_circuit_breaker_transitions() {
    let cb = CircuitBreaker::new(CircuitBreakerConfig::new().failure_threshold(3));
    assert_eq!(cb.state(), CircuitState::Closed);
    assert!(cb.is_allowed());

    for _ in 0..3 {
        cb.record_failure(true);
    }
    assert_eq!(cb.state(), CircuitState::Open);
    assert!(!cb.is_allowed());
    assert!(cb.stats().total_failures >= 3);

    let fixture = SqlTestFixture::new().await;
    let guarded = fixture
        .pool
        .clone()
        .with_circuit_breaker(CircuitBreakerConfig::new());
    assert_eq!(guarded.circuit_state(), CircuitState::Closed);
    assert!(guarded.circuit_stats().is_some());
    assert!(guarded.is_circuit_allowed());
}

#[tokio::test]
async fn test_metrics_counters_and_prometheus() {
    let metrics = SqlMetrics::with_defaults();
    metrics.record_retry("40001");
    metrics.record_retry("40P01");
    metrics.record_savepoint_created();

    let snap = metrics.snapshot();
    assert_eq!(snap.base.serialization_conflicts, 1);
    assert_eq!(snap.deadlocks_detected, 1);
    assert_eq!(snap.retry_attempts, 2);
    assert_eq!(snap.savepoints_created, 1);

    let prom = PrometheusMetrics::new("sql_test").expect("prometheus registry");
    let text = prom.encode().expect("encode");
    assert!(
        text.contains("sql_test"),
        "encoded output should include namespace"
    );
}

#[tokio::test]
async fn test_connect_with_retry_and_leak_detection() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = SqlPool::connect_with_retry(
        &SqlConfig::development(schema.url()).default_schema(schema.schema().clone()),
        3,
        Duration::from_millis(50),
    )
    .await
    .expect("connect_with_retry");
    assert!(pool.health_check().await.is_ok());

    {
        let _guard = pool.connection_guard();
    }
    assert_eq!(pool.leak_warning_count(), 0);
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, domain = "reports")]
struct Report {
    #[model(id)]
    id: uuid::Uuid,
    label: String,
}

#[tokio::test]
async fn test_domain_routes_to_named_connection() {
    // Two databases, since routing is between databases; neither minted, to spare a cluster-wide catalog write.
    let db_default = SqlSchema::mint().await.expect("the default database");
    let db_reports = SqlSchema::mint_on_a_second_database()
        .await
        .expect("a second database");
    // Two schemas in one database would still route, proving the wrong claim.
    assert_ne!(
        db_default.url(),
        db_reports.url(),
        "these must be two databases, or this test is not asking its own question"
    );

    let pool_default = db_default.pool().clone();
    let pool_reports = db_reports.pool().clone();
    for p in [&pool_default, &pool_reports] {
        p.execute_ddl(
            "CREATE TABLE IF NOT EXISTS reports (id UUID PRIMARY KEY, label TEXT NOT NULL)",
        )
        .await
        .unwrap();
    }

    let mut named = HashMap::new();
    named.insert("reports".to_string(), pool_reports.clone());
    let conns = SqlConnections::new(Some(pool_default.clone()), named);

    let repo = conns.repository::<Report>().expect("route to reports");
    repo.insert(&Report {
        id: uuid::Uuid::new_v4(),
        label: "q1".into(),
    })
    .await
    .expect("insert via routed repo");

    assert_eq!(
        pool_reports
            .fetch_all("SELECT id FROM reports")
            .await
            .unwrap()
            .len(),
        1,
        "row should be written to the reports connection"
    );
    assert_eq!(
        pool_default
            .fetch_all("SELECT id FROM reports")
            .await
            .unwrap()
            .len(),
        0,
        "default connection must be untouched"
    );

    assert!(conns.pool_for_domain("nonexistent").is_err());
}

/// A model with persisted `Secret` fields across all three at-rest shapes: text, binary and structured.
#[derive(Clone, Debug, Model)]
#[model(sql, table = "accounts")]
struct Account {
    #[model(id)]
    id: uuid::Uuid,
    email: String,
    #[model(encrypted = "pii")]
    api_token: SecretString,
    #[model(encrypted = "pii")]
    recovery_key: SecretBytes,
    #[model(encrypted = "pii")]
    profile: SecretJson<Vec<String>>,
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn secret_field_round_trips_as_ciphertext() {
    let fixture = SqlTestFixture::new().await;
    fixture
        .pool
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS accounts \
             (id UUID PRIMARY KEY, email TEXT NOT NULL, api_token TEXT NOT NULL, \
              recovery_key TEXT NOT NULL, profile TEXT NOT NULL)",
        )
        .await
        .expect("create accounts table");

    let km: fundamental_encryption::SharedKeyManager = Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[5u8; 32])
            .build()
            .expect("build key manager"),
    );
    let conns =
        SqlConnections::new(Some(fixture.pool.clone()), HashMap::new()).with_key_manager(km);
    let repo = conns.repository::<Account>().expect("repository");

    let id = uuid::Uuid::new_v4();
    let plaintext = "sk-live-0123456789";
    let key_bytes: Vec<u8> = vec![0x00, 0x01, 0x02, 0xff, 0xfe];
    let codes = vec!["a1b2".to_string(), "c3d4".to_string()];
    repo.insert(&Account {
        id,
        email: "a@example.com".to_string(),
        api_token: Secret::new(plaintext.to_string()),
        recovery_key: Secret::new(key_bytes.clone()),
        profile: Secret::new(codes.clone()),
    })
    .await
    .expect("insert account with secret");

    assert_eq!(
        <Account as fundamental_core::model::Model>::secret_columns(),
        &[
            ("api_token", "pii"),
            ("recovery_key", "pii"),
            ("profile", "pii"),
        ]
    );

    let row = fixture
        .pool
        .fetch_one(&format!(
            "SELECT api_token, recovery_key, profile FROM accounts WHERE id = '{id}'"
        ))
        .await
        .expect("fetch raw columns");
    for col in ["api_token", "recovery_key", "profile"] {
        let stored: String = sqlx::Row::try_get(&row, col).expect("read secret column");
        assert!(
            stored.contains("ciphertext"),
            "column {col} must store the envelope, got: {stored}"
        );
        assert!(
            !stored.contains(plaintext) && !stored.contains("a1b2"),
            "plaintext must never be persisted at rest in {col}"
        );
        assert!(
            !stored.contains("[REDACTED]"),
            "the redaction marker must never reach the database in {col}"
        );
    }

    let found = repo.find(&id).await.expect("find").expect("present");
    assert_eq!(found.api_token.expose().as_str(), plaintext);
    assert_eq!(found.recovery_key.expose(), &key_bytes);
    assert_eq!(found.profile.expose(), &codes);
    assert_eq!(format!("{:?}", found.api_token), "[REDACTED]");
    assert_eq!(format!("{:?}", found.recovery_key), "[REDACTED]");
    assert_eq!(format!("{:?}", found.profile), "[REDACTED]");
}

#[tokio::test]
async fn a_ciphertext_moved_to_another_row_refuses_to_decrypt() {
    let fixture = SqlTestFixture::new().await;
    fixture
        .pool
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS accounts \
             (id UUID PRIMARY KEY, email TEXT NOT NULL, api_token TEXT NOT NULL, \
              recovery_key TEXT NOT NULL, profile TEXT NOT NULL)",
        )
        .await
        .expect("create accounts table");
    let km: fundamental_encryption::SharedKeyManager = Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[5u8; 32])
            .build()
            .expect("build key manager"),
    );
    let conns =
        SqlConnections::new(Some(fixture.pool.clone()), HashMap::new()).with_key_manager(km);
    let repo = conns.repository::<Account>().expect("repository");

    let (owner, other) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    for (id, token) in [(owner, "sk-owner"), (other, "sk-other")] {
        repo.insert(&Account {
            id,
            email: "same@example.com".to_string(),
            api_token: Secret::new(token.to_string()),
            recovery_key: Secret::new(vec![1u8]),
            profile: Secret::new(vec!["p".to_string()]),
        })
        .await
        .expect("insert account");
    }
    fixture
        .pool
        .execute_ddl(&format!(
            "UPDATE accounts SET api_token = (SELECT api_token FROM accounts WHERE id = '{owner}') \
             WHERE id = '{other}'"
        ))
        .await
        .expect("move the envelope");

    let error = repo
        .find(&other)
        .await
        .expect_err("a moved ciphertext must not decrypt");
    assert_refused_decrypt(&error, "api_token");
    let owners = repo
        .find(&owner)
        .await
        .expect("the owner's row still opens");
    assert_eq!(
        owners.expect("present").api_token.expose().as_str(),
        "sk-owner"
    );
}

#[tokio::test]
async fn a_value_whose_bound_column_changed_refuses_to_decrypt() {
    let fixture = SqlTestFixture::new().await;
    fixture
        .pool
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS accounts \
             (id UUID PRIMARY KEY, email TEXT NOT NULL, api_token TEXT NOT NULL, \
              recovery_key TEXT NOT NULL, profile TEXT NOT NULL)",
        )
        .await
        .expect("create accounts table");
    let km: fundamental_encryption::SharedKeyManager = Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[5u8; 32])
            .build()
            .expect("build key manager"),
    );
    let conns =
        SqlConnections::new(Some(fixture.pool.clone()), HashMap::new()).with_key_manager(km);
    let repo = conns.repository::<Account>().expect("repository");

    let id = uuid::Uuid::new_v4();
    repo.insert(&Account {
        id,
        email: "a@example.com".to_string(),
        api_token: Secret::new("sk-live".to_string()),
        recovery_key: Secret::new(vec![1u8]),
        profile: Secret::new(vec!["p".to_string()]),
    })
    .await
    .expect("insert account");
    assert_eq!(
        <Account as fundamental_core::model::Model>::row_bound_columns(),
        &["id", "email"]
    );

    fixture
        .pool
        .execute_ddl(&format!(
            "UPDATE accounts SET email = 'b@example.com' WHERE id = '{id}'"
        ))
        .await
        .expect("edit the bound column behind the model's back");
    let error = repo
        .find(&id)
        .await
        .expect_err("a row whose bound column changed must not decrypt");
    assert_refused_decrypt(&error, "api_token");
}

fn assert_refused_decrypt(error: &fundamental_db::DatabaseError, column: &str) {
    let refused = match error {
        fundamental_db::DatabaseError::Other(inner) => inner.downcast_ref::<SqlError>(),
        _ => None,
    };
    assert!(
        matches!(
            refused,
            Some(SqlError::RowRefused {
                column: refused_column,
                kind: fundamental_core::outbox::RefusalKind::DecryptRefused,
                ..
            }) if *refused_column == column
        ),
        "expected a refused decrypt of '{column}', got {error:?}"
    );
}

#[tokio::test]
async fn a_sequenced_insert_of_a_model_with_encrypted_columns_is_refused() {
    let fixture = SqlTestFixture::new().await;
    let km: fundamental_encryption::SharedKeyManager = Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[5u8; 32])
            .build()
            .expect("build key manager"),
    );
    let conns =
        SqlConnections::new(Some(fixture.pool.clone()), HashMap::new()).with_key_manager(km);
    let repo = conns.repository::<Account>().expect("repository");
    let seq = SequenceRef {
        table: "seq_counter",
        key_column: "id",
        key: 0,
        next_column: "next_val",
    };
    let account = Account {
        id: uuid::Uuid::new_v4(),
        email: "a@example.com".to_string(),
        api_token: Secret::new("sk-live".to_string()),
        recovery_key: Secret::new(vec![1u8]),
        profile: Secret::new(vec!["p".to_string()]),
    };
    let error = repo
        .insert_or_get_sequenced(&account, "email", &seq)
        .await
        .expect_err("a sequence-assigned key cannot bind encrypted values");
    assert!(
        matches!(error, SqlError::Config(ref message) if message.contains("sequence-assigned key")),
        "expected a configuration refusal mentioning 'sequence-assigned key', got {error:?}"
    );
}

#[tokio::test]
async fn a_guarded_upsert_of_encrypted_values_must_conflict_on_the_key() {
    let fixture = SqlTestFixture::new().await;
    let km: fundamental_encryption::SharedKeyManager = Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[5u8; 32])
            .build()
            .expect("build key manager"),
    );
    let conns =
        SqlConnections::new(Some(fixture.pool.clone()), HashMap::new()).with_key_manager(km);
    let repo = conns.repository::<Account>().expect("repository");
    let account = Account {
        id: uuid::Uuid::new_v4(),
        email: "a@example.com".to_string(),
        api_token: Secret::new("sk-live".to_string()),
        recovery_key: Secret::new(vec![1u8]),
        profile: Secret::new(vec!["p".to_string()]),
    };
    let error = repo
        .upsert_guarded(&account, &["email"], &ColumnGuard::excluded_ge("email"))
        .await
        .expect_err("a non-key conflict target cannot carry encrypted values");
    assert!(
        matches!(error, SqlError::Config(ref message) if message.contains("must conflict on its key")),
        "expected a configuration refusal mentioning 'must conflict on its key', got {error:?}"
    );
}

fn assert_refused_config(error: &fundamental_db::DatabaseError, reason: &str) {
    let refused = match error {
        fundamental_db::DatabaseError::Other(inner) => inner.downcast_ref::<SqlError>(),
        _ => None,
    };
    assert!(
        matches!(refused, Some(SqlError::Config(message)) if message.contains(reason)),
        "expected a configuration refusal mentioning {reason:?}, got {error:?}"
    );
}

#[tokio::test]
async fn a_partial_update_that_would_unbind_a_row_is_refused() {
    let fixture = SqlTestFixture::new().await;
    let executor = fixture.pool.query_executor::<Account>();
    let every_account =
        QueryBuilder::<Account, SqlDb>::new().filter(Filter::eq("email", "a@example.com"));

    for column in ["email", "EMAIL"] {
        let rebind = executor
            .update_where(
                &every_account,
                &[(column.to_string(), FilterValue::from("b@example.com"))],
            )
            .await
            .expect_err("changing a bound column must be refused");
        assert_refused_config(&rebind, "bound into its encrypted values");
    }

    for column in ["api_token", "API_TOKEN"] {
        let plaintext = executor
            .update_where(
                &every_account,
                &[(column.to_string(), FilterValue::from("sk-plain"))],
            )
            .await
            .expect_err("writing plaintext into an encrypted column must be refused");
        assert_refused_config(&plaintext, "is encrypted");
    }
}

#[tokio::test]
async fn secret_insert_without_key_manager_is_a_typed_error() {
    let fixture = SqlTestFixture::new().await;
    fixture
        .pool
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS accounts \
             (id UUID PRIMARY KEY, email TEXT NOT NULL, api_token TEXT NOT NULL)",
        )
        .await
        .expect("create accounts table");

    let repo = fixture.pool.repository::<Account>();
    assert!(repo.key_manager().is_none());

    let result = repo
        .insert(&Account {
            id: uuid::Uuid::new_v4(),
            email: "b@example.com".to_string(),
            api_token: Secret::new("sk-live-xyz".to_string()),
            recovery_key: Secret::new(vec![1u8, 2, 3]),
            profile: Secret::new(vec!["x".to_string()]),
        })
        .await;
    assert!(
        result.is_err(),
        "inserting a Secret column with no key manager must error"
    );
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn secret_round_trips_through_a_registry_pool_and_transaction() {
    let fixture = SqlTestFixture::new().await;
    fixture
        .pool
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS accounts \
             (id UUID PRIMARY KEY, email TEXT NOT NULL, api_token TEXT NOT NULL, \
              recovery_key TEXT NOT NULL, profile TEXT NOT NULL)",
        )
        .await
        .expect("create accounts table");

    let km: fundamental_encryption::SharedKeyManager = Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[7u8; 32])
            .build()
            .expect("build key manager"),
    );
    let conns = SqlConnections::new(Some(fixture.pool.clone()), HashMap::new())
        .with_key_manager(km.clone());
    let pool = conns.pool_for_domain("default").expect("pool for default");
    assert!(
        pool.key_manager().is_some(),
        "a pool taken out of a km-configured registry must carry the km"
    );

    let id = uuid::Uuid::new_v4();
    let token = "sk-live-registry-pool";
    pool.repository::<Account>()
        .insert(&Account {
            id,
            email: "pool@example.com".to_string(),
            api_token: Secret::new(token.to_string()),
            recovery_key: Secret::new(vec![9u8, 8, 7]),
            profile: Secret::new(vec!["p".to_string()]),
        })
        .await
        .expect("pool repository insert encrypts the secret");

    let tx_id = uuid::Uuid::new_v4();
    let tx_token = "sk-live-registry-txn";
    pool.transaction(IsolationLevel::Serializable, async |tx| {
        tx.insert(&Account {
            id: tx_id,
            email: "txn@example.com".to_string(),
            api_token: Secret::new(tx_token.to_string()),
            recovery_key: Secret::new(vec![1u8, 2, 3]),
            profile: Secret::new(vec!["t".to_string()]),
        })
        .await?;
        Ok::<(), fundamental_db::DatabaseError>(())
    })
    .await
    .expect("transaction insert encrypts the secret");

    for row_id in [id, tx_id] {
        let row = fixture
            .pool
            .fetch_one(&format!(
                "SELECT api_token FROM accounts WHERE id = '{row_id}'"
            ))
            .await
            .expect("fetch raw column");
        let stored: String = sqlx::Row::try_get(&row, "api_token").expect("read column");
        assert!(stored.contains("ciphertext"), "must store the envelope");
        assert!(
            !stored.contains(token) && !stored.contains(tx_token),
            "plaintext must never be persisted"
        );
    }

    let via_repo = pool.repository::<Account>();
    assert_eq!(
        via_repo
            .find(&id)
            .await
            .expect("find")
            .expect("present")
            .api_token
            .expose()
            .as_str(),
        token
    );
    assert_eq!(
        via_repo
            .find(&tx_id)
            .await
            .expect("find")
            .expect("present")
            .api_token
            .expose()
            .as_str(),
        tx_token
    );
}

#[derive(Clone, Debug, Model)]
#[model(sql, table = "live_attributes")]
struct LiveAttribute {
    #[model(id)]
    id: uuid::Uuid,
    #[model(encrypted = "pii")]
    value: Option<SecretJson<serde_json::Value>>,
}

#[test]
fn a_nullable_secret_column_is_text_and_nullable_in_the_ddl() {
    let ddl = LiveAttribute::create_table_sql();
    assert!(
        ddl.contains("value TEXT"),
        "encrypted column is TEXT: {ddl}"
    );
    assert!(
        !ddl.contains("value TEXT NOT NULL"),
        "a nullable secret must be NULL-able: {ddl}"
    );
    assert_eq!(
        <LiveAttribute as fundamental_core::model::Model>::secret_columns(),
        &[("value", "pii")]
    );
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn a_nullable_secret_round_trips_present_and_absent() {
    let fixture = SqlTestFixture::new().await;
    fixture
        .pool
        .execute_ddl(LiveAttribute::create_table_sql())
        .await
        .expect("create live_attributes table");

    let km: fundamental_encryption::SharedKeyManager = Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[3u8; 32])
            .build()
            .expect("build key manager"),
    );
    let conns =
        SqlConnections::new(Some(fixture.pool.clone()), HashMap::new()).with_key_manager(km);
    let repo = conns.repository::<LiveAttribute>().expect("repository");

    let present_id = uuid::Uuid::new_v4();
    let value = serde_json::json!({ "country": "NL", "city": "Amsterdam" });
    repo.insert(&LiveAttribute {
        id: present_id,
        value: Some(Secret::new(value.clone())),
    })
    .await
    .expect("insert present value");

    let absent_id = uuid::Uuid::new_v4();
    repo.insert(&LiveAttribute {
        id: absent_id,
        value: None,
    })
    .await
    .expect("insert absent value");

    let present_raw: Option<String> = sqlx::Row::try_get(
        &fixture
            .pool
            .fetch_one(&format!(
                "SELECT value FROM live_attributes WHERE id = '{present_id}'"
            ))
            .await
            .expect("fetch present"),
        "value",
    )
    .expect("read column");
    let present_raw = present_raw.expect("present value is not NULL");
    assert!(
        present_raw.contains("ciphertext"),
        "present must be an envelope"
    );
    assert!(
        !present_raw.contains("Amsterdam"),
        "plaintext must never persist"
    );

    let absent_raw: Option<String> = sqlx::Row::try_get(
        &fixture
            .pool
            .fetch_one(&format!(
                "SELECT value FROM live_attributes WHERE id = '{absent_id}'"
            ))
            .await
            .expect("fetch absent"),
        "value",
    )
    .expect("read column");
    assert_eq!(absent_raw, None, "absent must be SQL NULL, not an envelope");

    let present = repo
        .find(&present_id)
        .await
        .expect("find")
        .expect("present");
    assert_eq!(present.value.expect("some").expose(), &value);
    let absent = repo.find(&absent_id).await.expect("find").expect("present");
    assert!(absent.value.is_none(), "absent decodes to None");

    fixture
        .pool
        .execute(&format!(
            "UPDATE live_attributes SET value = NULL WHERE id = '{present_id}'"
        ))
        .await
        .expect("erase present value");
    let erased = repo
        .find(&present_id)
        .await
        .expect("find")
        .expect("present");
    assert!(erased.value.is_none(), "an erased value reads back as None");
}

#[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
#[model(sql, table = "products")]
struct Product {
    #[model(id)]
    id: String,
    name: fundamental_core::i18n::Localized<String>,
    price: i64,
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn test_sql_localized_round_trip() {
    use fundamental_core::i18n::{Locale, Localized};
    use sqlx::Row as _;

    let fixture = SqlTestFixture::new().await;
    fixture
        .pool
        .execute_ddl(Product::create_table_sql())
        .await
        .expect("create products");
    let companion = <Product as fundamental_db::migration::ModelSchema<
        fundamental_core::model::SqlDb,
    >>::translation_schema()
    .expect("a localized record");
    for statement in fundamental_db::migration::render::create_all(&companion) {
        fixture
            .pool
            .execute_ddl(&statement)
            .await
            .expect("create products_i18n");
    }

    let mut name = Localized::new("Wireless Headphones");
    name.set("es", "Auriculares inalámbricos")
        .set("nl", "Draadloze koptelefoon");
    let product = Product {
        id: "p1".to_string(),
        name,
        price: 9999,
    };

    product.insert(&fixture.pool).await.expect("insert");
    let count: i64 = fixture
        .pool
        .fetch_one("SELECT count(*) FROM products_i18n")
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 2, "two companion rows written");

    let loaded = Product::find(&fixture.pool, &"p1".to_string())
        .await
        .expect("find")
        .expect("exists");
    assert_eq!(loaded, product);
    let es: Locale = "es".parse().unwrap();
    let de: Locale = "de".parse().unwrap();
    assert_eq!(loaded.name.resolve(&es), "Auriculares inalámbricos");
    assert_eq!(loaded.name.resolve(&de), "Wireless Headphones"); // → source

    let src: String = fixture
        .pool
        .fetch_one("SELECT name FROM products WHERE id = 'p1'")
        .await
        .unwrap()
        .get(0);
    assert_eq!(src, "Wireless Headphones");

    let hits = Product::find_ids_by_translation(&fixture.pool, "name", "es", "%inalámbricos%")
        .await
        .expect("search");
    assert_eq!(hits, ["p1".to_string()]);
    let misses = Product::find_ids_by_translation(&fixture.pool, "name", "es", "%nope%")
        .await
        .expect("search");
    assert!(misses.is_empty());

    let updated = Product {
        id: "p1".to_string(),
        name: Localized::new("Wireless Headphones").with("es", "Auriculares premium"),
        price: 7999,
    };
    updated.upsert(&fixture.pool).await.expect("upsert");
    let reloaded = Product::find(&fixture.pool, &"p1".to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reloaded.price, 7999, "main row updated");
    assert_eq!(
        reloaded.name.resolve(&es),
        "Auriculares premium",
        "translation updated"
    );
    let after_upsert: i64 = fixture
        .pool
        .fetch_one("SELECT count(*) FROM products_i18n")
        .await
        .unwrap()
        .get(0);
    assert_eq!(after_upsert, 1, "the nl override was dropped on upsert");

    let mut p = Product::find(&fixture.pool, &"p1".to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(
        p.name
            .to_check(&fundamental_core::i18n::Languages::new(
                "en".parse().unwrap(),
                ["es".parse().unwrap()]
            ))
            .is_empty(),
        "fresh translation is not stale"
    );
    p.name.set_source("Wired Headphones");
    p.upsert(&fixture.pool).await.expect("upsert source change");
    let p = Product::find(&fixture.pool, &"p1".to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        p.name.to_check(&fundamental_core::i18n::Languages::new(
            "en".parse().unwrap(),
            ["es".parse().unwrap()]
        )),
        ["es".parse::<fundamental_core::i18n::Locale>().unwrap()],
        "es is stale after the source changed"
    );

    let deleted = Product::delete(&fixture.pool, &"p1".to_string())
        .await
        .expect("delete");
    assert_eq!(deleted, 1);
    assert!(
        Product::find(&fixture.pool, &"p1".to_string())
            .await
            .unwrap()
            .is_none()
    );
    let remaining: i64 = fixture
        .pool
        .fetch_one("SELECT count(*) FROM products_i18n")
        .await
        .unwrap()
        .get(0);
    assert_eq!(remaining, 0, "companion rows cascade-deleted");
}

#[tokio::test]
async fn test_repository_ext_find_or_create_and_update_if_exists() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;
    let repo = fixture.pool.repository::<TestUser>();

    let id = uuid::Uuid::new_v4();

    let created = repo
        .find_or_create(&id, || TestUser {
            id,
            email: "foc@example.com".into(),
            name: "Created".into(),
        })
        .await
        .expect("find_or_create create");
    assert_eq!(created.name, "Created");
    assert_eq!(RepositoryAggregate::count(&repo).await.unwrap(), 1);

    let again = repo
        .find_or_create(&id, || panic!("factory must not run when the row exists"))
        .await
        .expect("find_or_create hit");
    assert_eq!(again, created);
    assert_eq!(RepositoryAggregate::count(&repo).await.unwrap(), 1);

    let updated = repo
        .update_if_exists(&TestUser {
            name: "Updated".into(),
            ..created.clone()
        })
        .await
        .expect("update_if_exists present");
    assert_eq!(updated.expect("some").name, "Updated");
    assert_eq!(repo.find(&id).await.unwrap().unwrap().name, "Updated");

    let missing = repo
        .update_if_exists(&TestUser {
            id: uuid::Uuid::new_v4(),
            email: "ghost@example.com".into(),
            name: "Ghost".into(),
        })
        .await
        .expect("update_if_exists absent");
    assert!(missing.is_none());
    assert_eq!(RepositoryAggregate::count(&repo).await.unwrap(), 1);
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "soft_docs", soft_delete)]
struct SoftDoc {
    #[model(id)]
    id: uuid::Uuid,
    body: String,
}

#[tokio::test]
async fn test_repository_soft_delete_round_trip() {
    use sqlx::Row as _;

    let fixture = SqlTestFixture::new().await;
    fixture
        .pool
        .execute_ddl(SoftDoc::create_table_sql())
        .await
        .expect("create soft_docs");
    let repo = fixture.pool.repository::<SoftDoc>();

    let id = uuid::Uuid::new_v4();
    repo.insert(&SoftDoc {
        id,
        body: "hello".into(),
    })
    .await
    .expect("insert");

    assert!(repo.soft_delete(&id).await.expect("soft_delete"));
    assert!(repo.find(&id).await.expect("find").is_none());
    assert!(
        repo.find_with_deleted(&id)
            .await
            .expect("find_with_deleted")
            .is_some()
    );
    assert!(!repo.soft_delete(&id).await.expect("soft_delete again"));

    let stamped: i64 = fixture
        .pool
        .fetch_one("SELECT count(*) FROM soft_docs WHERE deleted_at IS NOT NULL")
        .await
        .unwrap()
        .get(0);
    assert_eq!(stamped, 1, "deleted_at stamped after soft_delete");

    assert!(repo.restore(&id).await.expect("restore"));
    assert!(repo.find(&id).await.expect("find").is_some());
    let cleared: i64 = fixture
        .pool
        .fetch_one("SELECT count(*) FROM soft_docs WHERE deleted_at IS NOT NULL")
        .await
        .unwrap()
        .get(0);
    assert_eq!(cleared, 0, "deleted_at cleared after restore");

    assert!(repo.purge(&id).await.expect("purge"));
    assert!(
        repo.find_with_deleted(&id)
            .await
            .expect("find_with_deleted after purge")
            .is_none()
    );
    assert!(!repo.purge(&id).await.expect("purge again"));
}

#[tokio::test]
async fn test_count_where_filtered() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;
    let repo = fixture.pool.repository::<TestUser>();
    seed_users(&repo, &["Alice", "Alice", "Bob"]).await;

    let alices = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("name", "Alice"));
    assert_eq!(repo.count_where(&alices).await.expect("count_where"), 2);
    assert_eq!(RepositoryAggregate::count(&repo).await.expect("count"), 3);

    let nobody = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("name", "Nobody"));
    assert_eq!(
        repo.count_where(&nobody).await.expect("count_where empty"),
        0
    );
}

#[tokio::test]
async fn tx_query_where_binds_uuid_and_delete_where_is_transactional() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;
    let repo = fixture.pool.repository::<TestUser>();
    seed_users(&repo, &["Alice", "Alice", "Bob"]).await;

    let alices = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("name", "Alice"));
    let an_alice_id = fixture
        .pool
        .query_executor::<TestUser>()
        .execute(&alices)
        .await
        .expect("seed read")
        .first()
        .expect("an alice exists")
        .id;

    let rolled_back: Result<(), DatabaseError> = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            let by_id =
                QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("id", an_alice_id));
            let hit = RepositoryTransactionQuery::query_where(tx, &by_id).await?;
            assert_eq!(hit.len(), 1, "Uuid filter matches exactly the one row");
            assert_eq!(hit[0].id, an_alice_id);

            let removed = RepositoryTransactionQuery::delete_where(tx, &alices).await?;
            assert_eq!(removed, 2, "both Alices deleted inside the tx");

            let after = RepositoryTransactionQuery::query_where(tx, &alices).await?;
            assert!(
                after.is_empty(),
                "no Alices visible inside the tx after delete"
            );

            Err(DatabaseError::query("force rollback"))
        })
        .await;
    assert!(rolled_back.is_err(), "closure error propagates");
    assert_eq!(
        RepositoryAggregate::count(&repo)
            .await
            .expect("count after rollback"),
        3
    );

    let removed: Result<u64, DatabaseError> = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            RepositoryTransactionQuery::delete_where(tx, &alices).await
        })
        .await;
    assert_eq!(removed.expect("commit"), 2);
    assert_eq!(
        RepositoryAggregate::count(&repo)
            .await
            .expect("count after commit"),
        1
    );
}

#[tokio::test]
async fn tx_update_where_is_transactional() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;
    let repo = fixture.pool.repository::<TestUser>();
    seed_users(&repo, &["Alice", "Alice", "Bob"]).await;

    let alices = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("name", "Alice"));
    let alicias = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("name", "Alicia"));
    let rename = [("name".to_string(), FilterValue::from("Alicia"))];
    let executor = fixture.pool.query_executor::<TestUser>();

    let rolled_back: Result<(), DatabaseError> = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            let updated = RepositoryTransactionQuery::update_where(tx, &alices, &rename).await?;
            assert_eq!(updated, 2, "both Alices renamed inside the tx");

            let after = RepositoryTransactionQuery::query_where(tx, &alicias).await?;
            assert_eq!(after.len(), 2, "the rename is visible inside the tx");

            Err(DatabaseError::query("force rollback"))
        })
        .await;
    assert!(rolled_back.is_err(), "closure error propagates");
    assert_eq!(
        executor.count(&alices).await.expect("count after rollback"),
        2,
        "the update rolled back — the Alices are unchanged"
    );
    assert_eq!(executor.count(&alicias).await.expect("count"), 0);

    let updated: Result<u64, DatabaseError> = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            RepositoryTransactionQuery::update_where(tx, &alices, &rename).await
        })
        .await;
    assert_eq!(updated.expect("commit"), 2);
    assert_eq!(
        executor.count(&alices).await.expect("count after commit"),
        0
    );
    assert_eq!(executor.count(&alicias).await.expect("count"), 2);
}

#[tokio::test]
async fn test_find_required_missing_is_not_found() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;
    let repo = fixture.pool.repository::<TestUser>();

    let user = TestUser {
        id: uuid::Uuid::new_v4(),
        email: "req@example.com".into(),
        name: "Req".into(),
    };
    repo.insert(&user).await.expect("insert");

    assert_eq!(repo.find_required(&user.id).await.expect("present"), user);

    let err = repo
        .find_required(&uuid::Uuid::new_v4())
        .await
        .expect_err("missing must error");
    assert_eq!(err.error_code(), "DB_004");
    assert_eq!(err.http_status_code(), 404);
}

#[derive(Clone, Debug, Model)]
#[model(sql, table = "widgets")]
struct Widget {
    #[model(id)]
    id: uuid::Uuid,
    name: String,
    count: Option<i64>,
}

#[tokio::test]
async fn introspected_schema_matches_the_model() {
    let fixture = SqlTestFixture::new().await;
    fixture
        .pool
        .execute_ddl(Widget::create_table_sql())
        .await
        .expect("create widgets table");

    let actual = SqlIntrospector::introspect(&fixture.pool)
        .await
        .expect("introspect the live schema");

    let mut expected = DatabaseSchema::for_database(MigrationDatabase::SqlDb);
    expected.add_table(<Widget as ModelSchema<SqlDb>>::table_schema());

    let changes = SchemaDiffer::new().diff(&expected, &actual);
    assert!(
        changes.is_empty(),
        "model and freshly-created table must match exactly, found: {changes:?}"
    );
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn slice_store_reads_scoped_changed_rows() {
    let fixture = SqlTestFixture::new().await;

    fixture
        .pool
        .execute_ddl(
            "CREATE TABLE animals (\
                 id UUID PRIMARY KEY DEFAULT gen_random_uuid(),\
                 name TEXT NOT NULL,\
                 owner_id TEXT NOT NULL,\
                 updated_at TIMESTAMPTZ NOT NULL,\
                 version BIGINT NOT NULL,\
                 _tombstone BOOL NOT NULL)",
        )
        .await
        .expect("create animals table");

    fixture
        .pool
        .execute(
            "INSERT INTO animals (name, owner_id, updated_at, version, _tombstone) VALUES \
                 ('cat','alice', to_timestamp(1.0), 1, false),\
                 ('dog','alice', to_timestamp(3.0), 2, true),\
                 ('fish','bob',  to_timestamp(2.0), 1, false)",
        )
        .await
        .expect("seed animals");

    let store = SqlSliceStore::new(&fixture.pool);
    let columns = ["name", "owner_id"];

    let alice = store
        .changed_since(
            "animals",
            &columns,
            &["id"],
            &[Filter::eq("owner_id", "alice")],
            0,
        )
        .await
        .expect("changed_since for alice");

    let names: Vec<&str> = alice.iter().map(|r| r.payload["name"].as_str()).collect();
    assert_eq!(names, vec!["cat", "dog"], "scoped + ordered by updated_at");
    assert!(
        alice.iter().all(|r| r.payload["owner_id"] == "alice"),
        "no cross-principal rows"
    );

    let dog = &alice[1];
    assert_eq!(dog.meta.updated_at, 3000, "updated_at rendered as epoch ms");
    assert_eq!(dog.meta.version, 2);
    assert!(dog.meta.tombstone, "a propagated delete is a tombstone");
    assert!(
        !dog.id.is_empty(),
        "the primary key travels as the record id"
    );

    let after_cat = store
        .changed_since(
            "animals",
            &columns,
            &["id"],
            &[Filter::eq("owner_id", "alice")],
            1000,
        )
        .await
        .expect("changed_since after the cat");
    let names: Vec<&str> = after_cat
        .iter()
        .map(|r| r.payload["name"].as_str())
        .collect();
    assert_eq!(
        names,
        vec!["dog"],
        "high-water mark is strictly greater-than"
    );
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn slice_store_upsert_round_trips_typed_columns() {
    let fixture = SqlTestFixture::new().await;

    fixture
        .pool
        .execute_ddl(
            "CREATE TABLE widgets (\
                 id UUID PRIMARY KEY,\
                 label TEXT NOT NULL,\
                 qty INT8 NOT NULL,\
                 note TEXT,\
                 due TIMESTAMPTZ,\
                 owner_id TEXT NOT NULL,\
                 updated_at TIMESTAMPTZ NOT NULL,\
                 version BIGINT NOT NULL,\
                 _tombstone BOOL NOT NULL)",
        )
        .await
        .expect("create widgets table");

    let store = SqlSliceStore::new(&fixture.pool);
    let cols = ["label", "qty", "note", "due", "owner_id"];
    let id = "11111111-1111-1111-1111-111111111111";

    let widget = |label: &str, qty: &str, owner: &str, note: Option<&str>, due: Option<&str>| {
        let mut payload = BTreeMap::new();
        payload.insert("label".to_string(), label.to_string());
        payload.insert("qty".to_string(), qty.to_string());
        payload.insert("owner_id".to_string(), owner.to_string());
        if let Some(note) = note {
            payload.insert("note".to_string(), note.to_string());
        }
        if let Some(due) = due {
            payload.insert("due".to_string(), due.to_string());
        }
        SyncRecord::new(id, SyncMeta::new(0, 1), payload)
    };

    let fetch = || async {
        store
            .changed_since(
                "widgets",
                &cols,
                &["id"],
                &[Filter::eq("owner_id", "alice")],
                0,
            )
            .await
            .expect("changed_since")
    };

    store
        .upsert(
            "widgets",
            &cols,
            &["id"],
            &widget("hi", "7", "alice", None, Some("2024-06-01T12:00:00Z")),
        )
        .await
        .expect("insert upsert");

    let rows = fetch().await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, id, "uuid primary key round-trips as canonical text");
    assert_eq!(row.payload["label"], "hi");
    assert_eq!(row.payload["qty"], "7", "int8 round-trips");
    assert_eq!(row.payload["owner_id"], "alice");
    assert!(
        row.payload["due"].starts_with("2024-06-01"),
        "timestamptz round-trips: {}",
        row.payload["due"]
    );
    assert!(
        !row.payload.contains_key("note"),
        "a NULL column is omitted, not empty"
    );
    assert_eq!(row.meta.version, 1, "version stamped to 1 on insert");
    assert!(!row.meta.tombstone);
    assert!(
        row.meta.updated_at > 1_700_000_000_000,
        "updated_at stamped server-side"
    );

    store
        .upsert(
            "widgets",
            &cols,
            &["id"],
            &widget("hi", "9", "alice", Some("memo"), None),
        )
        .await
        .expect("update upsert");

    let row = fetch().await.remove(0);
    assert_eq!(row.payload["qty"], "9");
    assert_eq!(
        row.payload["note"], "memo",
        "a previously-NULL column is now set"
    );
    assert!(
        !row.payload.contains_key("due"),
        "a column set back to NULL is omitted"
    );
    assert_eq!(row.meta.version, 2, "version bumped on conflict");

    let tombstone = SyncRecord::new(id, SyncMeta::new(0, 0).tombstoned(), BTreeMap::new());
    store
        .upsert("widgets", &cols, &["id"], &tombstone)
        .await
        .expect("tombstone upsert");

    let row = fetch().await.remove(0);
    assert!(row.meta.tombstone, "the row is now a tombstone");
    assert_eq!(row.meta.version, 3);
    assert_eq!(
        row.payload["qty"], "9",
        "tombstone preserves the row's data"
    );
}

#[tokio::test]
async fn slice_store_changed_page_keyset_paginates() {
    use fundamental_sync::wire::SliceCursor;

    let fixture = SqlTestFixture::new().await;
    fixture
        .pool
        .execute_ddl(
            "CREATE TABLE pages (\
                 id UUID PRIMARY KEY,\
                 owner_id TEXT NOT NULL,\
                 updated_at TIMESTAMPTZ NOT NULL,\
                 version BIGINT NOT NULL,\
                 _tombstone BOOL NOT NULL)",
        )
        .await
        .expect("create pages table");

    fixture
        .pool
        .execute(
            "INSERT INTO pages (id, owner_id, updated_at, version, _tombstone) VALUES \
                 ('00000000-0000-0000-0000-000000000001','alice', to_timestamp(1.0), 1, false),\
                 ('00000000-0000-0000-0000-000000000002','alice', to_timestamp(1.0), 1, false),\
                 ('00000000-0000-0000-0000-000000000003','alice', to_timestamp(1.0), 1, false),\
                 ('00000000-0000-0000-0000-000000000004','alice', to_timestamp(2.0), 1, false),\
                 ('00000000-0000-0000-0000-000000000005','alice', to_timestamp(3.0), 1, false)",
        )
        .await
        .expect("seed pages");

    let store = SqlSliceStore::new(&fixture.pool);
    let filters = [Filter::eq("owner_id", "alice")];

    let mut all = Vec::new();
    let mut cursor: Option<SliceCursor> = None;
    loop {
        let page = store
            .changed_page("pages", &[], &["id"], &filters, 0, cursor.as_ref(), 2)
            .await
            .expect("changed_page");
        if page.is_empty() {
            break;
        }
        cursor = page
            .last()
            .map(|r| SliceCursor::new(r.meta.updated_at, r.id.clone()));
        all.extend(page.into_iter().map(|r| r.id));
        if all.len() > 10 {
            panic!("paging did not terminate");
        }
    }

    assert_eq!(
        all,
        vec![
            "00000000-0000-0000-0000-000000000001".to_string(),
            "00000000-0000-0000-0000-000000000002".to_string(),
            "00000000-0000-0000-0000-000000000003".to_string(),
            "00000000-0000-0000-0000-000000000004".to_string(),
            "00000000-0000-0000-0000-000000000005".to_string(),
        ],
        "every row exactly once, in (updated_at, id) order, across the shared-timestamp boundary",
    );
}

/// The same paging on an `INT8` key, where `id::text > '9'` is false for `'10'`.
#[tokio::test]
async fn slice_store_paging_resumes_at_the_key_type_not_its_text() {
    use fundamental_sync::wire::SliceCursor;

    let fixture = SqlTestFixture::new().await;
    fixture
        .pool
        .execute_ddl(
            "CREATE TABLE counted (\
                 id INT8 PRIMARY KEY,\
                 owner_id TEXT NOT NULL,\
                 updated_at TIMESTAMPTZ NOT NULL,\
                 version BIGINT NOT NULL,\
                 _tombstone BOOL NOT NULL)",
        )
        .await
        .expect("create counted table");

    // One shared `updated_at`, so every page boundary falls on the key comparison.
    fixture
        .pool
        .execute(
            "INSERT INTO counted (id, owner_id, updated_at, version, _tombstone) VALUES \
                 (8,'alice', to_timestamp(1.0), 1, false),\
                 (9,'alice', to_timestamp(1.0), 1, false),\
                 (10,'alice', to_timestamp(1.0), 1, false),\
                 (11,'alice', to_timestamp(1.0), 1, false),\
                 (100,'alice', to_timestamp(1.0), 1, false)",
        )
        .await
        .expect("seed counted");

    let store = SqlSliceStore::new(&fixture.pool);
    let filters = [Filter::eq("owner_id", "alice")];

    let mut all = Vec::new();
    let mut cursor: Option<SliceCursor> = None;
    loop {
        let page = store
            .changed_page("counted", &[], &["id"], &filters, 0, cursor.as_ref(), 1)
            .await
            .expect("changed_page");
        if page.is_empty() {
            break;
        }
        cursor = page
            .last()
            .map(|r| SliceCursor::new(r.meta.updated_at, r.id.clone()));
        all.extend(page.into_iter().map(|r| r.id));
        assert!(all.len() <= 5, "paging did not terminate: {all:?}");
    }

    assert_eq!(
        all,
        vec![
            "8".to_string(),
            "9".to_string(),
            "10".to_string(),
            "11".to_string(),
            "100".to_string(),
        ],
        "numeric key order, with no row skipped at the 9→10 or 11→100 text boundary",
    );
}

/// Compile-time proof that `SqlTransaction` implements the typed transaction-repository surface.
fn _assert_tx_repository_traits()
where
    fundamental_sql::SqlTransaction: RepositoryTransaction<TestUser, SqlDb, Id = uuid::Uuid>
        + RepositoryTransactionBatch<TestUser, SqlDb>
        + KeyedRepositoryTransactionBatch<TestUser, SqlDb>
        + RepositoryTransactionAggregate<TestUser, SqlDb>
        + RepositoryTransactionExt<TestUser, SqlDb>
        + RepositoryTransactionSoftDelete<TestUser, SqlDb>
        + RepositoryTransactionTranslation<TestUser, SqlDb>,
{
}

#[tokio::test]
async fn test_tx_repository_commits() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;

    let alice = TestUser {
        id: uuid::Uuid::new_v4(),
        email: "tx_alice@example.com".into(),
        name: "Alice".into(),
    };
    let bob = TestUser {
        id: uuid::Uuid::new_v4(),
        email: "tx_bob@example.com".into(),
        name: "Bob".into(),
    };

    fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.insert(&alice).await?;
            tx.insert(&bob).await?;
            Ok::<(), DatabaseError>(())
        })
        .await
        .expect("typed tx should commit");

    let repo = fixture.pool.repository::<TestUser>();
    assert_eq!(RepositoryAggregate::count(&repo).await.unwrap(), 2);
    assert_eq!(repo.find(&alice.id).await.unwrap(), Some(alice));
}

#[tokio::test]
async fn test_tx_repository_reads_its_own_writes() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;

    let user = TestUser {
        id: uuid::Uuid::new_v4(),
        email: "tx_ryow@example.com".into(),
        name: "RYOW".into(),
    };

    let (existed, name) = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.insert(&user).await?;
            let found: Option<TestUser> = tx.find(&user.id).await?;
            let existed =
                KeyedRepositoryTransaction::<TestUser, SqlDb>::exists(tx, &user.id).await?;
            Ok::<(bool, String), DatabaseError>((
                existed,
                found.map(|u| u.name).unwrap_or_default(),
            ))
        })
        .await
        .expect("typed tx should commit");

    assert!(
        existed,
        "the insert is visible to a later typed read in the same tx"
    );
    assert_eq!(name, "RYOW");
}

#[tokio::test]
async fn test_tx_repository_rolls_back_on_err() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;

    let user = TestUser {
        id: uuid::Uuid::new_v4(),
        email: "tx_rollback@example.com".into(),
        name: "Rollback".into(),
    };

    let outcome: Result<(), DatabaseError> = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.insert(&user).await?;
            Err(DatabaseError::query("boom"))
        })
        .await;

    let err = outcome.expect_err("closure error must propagate");
    assert_eq!(err.error_code(), "DB_002");

    let repo = fixture.pool.repository::<TestUser>();
    assert_eq!(
        RepositoryAggregate::count(&repo).await.unwrap(),
        0,
        "the typed write is rolled back after the error"
    );
}

#[tokio::test]
async fn test_tx_repository_batch_and_interleaving() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_test_users().await;

    let users: Vec<TestUser> = (0..3)
        .map(|i| TestUser {
            id: uuid::Uuid::new_v4(),
            email: format!("tx_batch_{i}@example.com"),
            name: format!("Batch {i}"),
        })
        .collect();
    let ids: Vec<uuid::Uuid> = users.iter().map(|u| u.id).collect();

    let found_len = fixture
        .pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.insert_many(&users).await?;
            let extra = TestUser {
                id: uuid::Uuid::new_v4(),
                email: "tx_batch_extra@example.com".into(),
                name: "Extra".into(),
            };
            tx.insert(&extra).await?;
            let found: Vec<TestUser> = tx.find_many(&ids).await?;
            Ok::<usize, DatabaseError>(found.len())
        })
        .await
        .expect("typed batch tx should commit");

    assert_eq!(found_len, 3, "find_many returns the batch-inserted rows");
    assert_eq!(
        RepositoryAggregate::count(&fixture.pool.repository::<TestUser>())
            .await
            .unwrap(),
        4
    );
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct ZonedRow {
    #[model(id)]
    zone: String,
    #[model(id)]
    row_id: uuid::Uuid,
    payload: String,
}

impl SqlTestFixture {
    async fn create_zoned_rows(&self) {
        self.pool
            .execute_ddl(
                "CREATE TABLE IF NOT EXISTS zoned_rows (\
                 zone TEXT NOT NULL, \
                 row_id UUID NOT NULL, \
                 payload TEXT NOT NULL, \
                 PRIMARY KEY (zone, row_id))",
            )
            .await
            .expect("create zoned_rows");
    }
}

#[tokio::test]
async fn an_upsert_of_a_composite_key_touches_only_its_own_row() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_zoned_rows().await;
    let repo = fixture.pool.repository::<ZonedRow>();

    let shared_id = uuid::Uuid::new_v4();
    let eu = ZonedRow {
        zone: "eu".into(),
        row_id: shared_id,
        payload: "eu-before".into(),
    };
    let uk = ZonedRow {
        zone: "uk".into(),
        row_id: shared_id,
        payload: "uk-before".into(),
    };
    repo.insert_many(&[eu.clone(), uk.clone()])
        .await
        .expect("insert both zones");

    repo.upsert_many(&[ZonedRow {
        payload: "eu-after".into(),
        ..eu.clone()
    }])
    .await
    .expect("upsert one zone");

    let after_eu =
        SqlModelQuery::<ZonedRow>::find_by_key(&fixture.pool, &("eu".to_string(), shared_id))
            .await
            .expect("read eu back")
            .expect("the eu row exists");
    let after_uk =
        SqlModelQuery::<ZonedRow>::find_by_key(&fixture.pool, &("uk".to_string(), shared_id))
            .await
            .expect("read uk back")
            .expect("the uk row exists");
    assert_eq!(after_eu.payload, "eu-after");
    assert_eq!(after_uk, uk, "the other zone's row must be untouched");
}

/// The same `row_id` in two zones: only the whole key distinguishes them.
#[tokio::test]
async fn a_batch_update_of_a_composite_key_touches_only_its_own_tenancy() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_zoned_rows().await;
    let repo = fixture.pool.repository::<ZonedRow>();

    let shared_id = uuid::Uuid::new_v4();
    let eu = ZonedRow {
        zone: "eu".into(),
        row_id: shared_id,
        payload: "eu-before".into(),
    };
    let uk = ZonedRow {
        zone: "uk".into(),
        row_id: shared_id,
        payload: "uk-before".into(),
    };
    repo.insert_many(&[eu.clone(), uk.clone()])
        .await
        .expect("insert both zones");

    let updated = repo
        .update_many(&[ZonedRow {
            payload: "eu-after".into(),
            ..eu.clone()
        }])
        .await
        .expect("batch update one zone");
    assert_eq!(updated.len(), 1, "one row addressed, one row returned");
    assert_eq!(updated[0].zone, "eu");

    let key_eu = ("eu".to_string(), shared_id);
    let key_uk = ("uk".to_string(), shared_id);
    let after_eu = SqlModelQuery::<ZonedRow>::find_by_key(&fixture.pool, &key_eu)
        .await
        .expect("read eu back")
        .expect("the eu row exists");
    let after_uk = SqlModelQuery::<ZonedRow>::find_by_key(&fixture.pool, &key_uk)
        .await
        .expect("read uk back")
        .expect("the uk row exists");

    assert_eq!(
        after_eu.payload, "eu-after",
        "the addressed row was updated"
    );
    assert_eq!(
        after_uk.payload, "uk-before",
        "a row in another jurisdiction sharing the id must be untouched"
    );
}

/// Every `CASE` ends `ELSE {col}`, so an unmatched row keeps its value instead of `NULL`.
#[tokio::test]
async fn a_batch_update_gives_each_row_its_own_value() {
    let fixture = SqlTestFixture::new().await;
    fixture.create_zoned_rows().await;
    let repo = fixture.pool.repository::<ZonedRow>();

    let first = ZonedRow {
        zone: "eu".into(),
        row_id: uuid::Uuid::new_v4(),
        payload: "one".into(),
    };
    let second = ZonedRow {
        zone: "eu".into(),
        row_id: uuid::Uuid::new_v4(),
        payload: "two".into(),
    };
    repo.insert_many(&[first.clone(), second.clone()])
        .await
        .expect("insert two rows");

    let updated = repo
        .update_many(&[
            ZonedRow {
                payload: "one-new".into(),
                ..first.clone()
            },
            ZonedRow {
                payload: "two-new".into(),
                ..second.clone()
            },
        ])
        .await
        .expect("batch update two rows");
    assert_eq!(updated.len(), 2);

    for (row, expected) in [(&first, "one-new"), (&second, "two-new")] {
        let key = (row.zone.clone(), row.row_id);
        let back = SqlModelQuery::<ZonedRow>::find_by_key(&fixture.pool, &key)
            .await
            .expect("read back")
            .expect("row exists");
        assert_eq!(
            back.payload, expected,
            "each row in a batch takes its own value, never a neighbour's and never NULL"
        );
    }
}

/// The saturation signal moves, and moves before the timeout counter does.
mod pool_saturation {
    use std::sync::Arc;
    use std::time::Duration;

    use fundamental_db::ConnectionPool;
    use fundamental_sql::{PrometheusMetrics, SqlConfig, SqlPool};
    use fundamental_test_containers::sql::SqlSchema;

    /// A pool whose ceiling is one connection, so the second caller is a waiter deterministically.
    async fn one_connection_pool(schema: &SqlSchema, acquire_timeout: Duration) -> SqlPool {
        SqlPool::connect(
            &SqlConfig::development(schema.url())
                .default_schema(schema.schema().clone())
                .max_connections(1)
                .acquire_timeout(acquire_timeout),
        )
        .await
        .expect("a pool against the test schema")
    }

    /// Waits for `condition` to hold, bounded so a property that never holds fails rather than hangs.
    async fn settles(mut condition: impl FnMut() -> bool) -> bool {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !condition() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_ok()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn a_waiter_is_counted_while_it_waits_and_released_when_it_is_served() {
        let schema = SqlSchema::mint().await.expect("a test schema");
        // Long enough that the waiter waits rather than times out.
        let pool = one_connection_pool(&schema, Duration::from_secs(30)).await;

        assert_eq!(
            pool.status().pending,
            Some(0),
            "an idle pool reports a measured zero, not an absent number"
        );

        let held = pool
            .connector()
            .acquire()
            .await
            .expect("the one connection");

        let waiting = tokio::spawn({
            let pool = pool.clone();
            async move { pool.connector().acquire().await.map(|_| ()) }
        });

        assert!(
            settles(|| pool.status().pending == Some(1)).await,
            "a caller queued behind the only connection was never counted; pending stayed {:?}",
            pool.status().pending
        );
        assert_eq!(
            pool.status().is_saturated(),
            Some(true),
            "the pool is at its ceiling while the waiter waits"
        );

        drop(held);
        waiting
            .await
            .expect("the waiting task ran")
            .expect("the waiter was served once the connection came back");
        assert!(
            settles(|| pool.status().pending == Some(0)).await,
            "the waiter was served and is still counted as waiting"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_acquire_that_times_out_moves_the_counter_a_critical_alert_reads() {
        let schema = SqlSchema::mint().await.expect("a test schema");
        let pool = one_connection_pool(&schema, Duration::from_millis(250)).await;
        let metrics = Arc::new(PrometheusMetrics::new("saturation").expect("a registry"));
        let pool = pool.with_metrics(metrics.clone());

        let held = pool
            .connector()
            .acquire()
            .await
            .expect("the one connection");
        let refused = pool.connector().acquire().await;
        drop(held);

        assert!(
            refused.is_err(),
            "a second acquire against a ceiling of one must not succeed"
        );
        let scraped = metrics.encode().expect("the registry renders");
        assert!(
            scraped.contains("saturation_sql_connection_timeouts_total 1"),
            "the timeout counter did not move, so the critical alert on it still cannot \
             fire:\n{scraped}"
        );
        assert!(
            scraped.contains(
                "saturation_sql_connection_acquire_duration_seconds_count{success=\"true\"} 1"
            ),
            "the served acquisition was not timed:\n{scraped}"
        );
        assert!(
            scraped.contains(
                "saturation_sql_connection_acquire_duration_seconds_count{success=\"false\"} 1"
            ),
            "the refused acquisition was not timed:\n{scraped}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refused_acquire_is_a_typed_retryable_503() {
        use fundamental_core::Error;

        let schema = SqlSchema::mint().await.expect("a test schema");
        let pool = one_connection_pool(&schema, Duration::from_millis(250)).await;

        let held = pool
            .connector()
            .acquire()
            .await
            .expect("the one connection");
        let refused = pool
            .connector()
            .acquire()
            .await
            .expect_err("a second acquire against a ceiling of one must not succeed");
        drop(held);

        assert_eq!(refused.error_code(), "DB_007");
        assert_eq!(refused.http_status_code(), 503);
        assert!(
            refused.is_retryable(),
            "the pool drains; the caller may return"
        );
        assert!(
            !refused.should_retry_in_process(),
            "asking a saturated pool again is the pressure that saturated it"
        );
        assert!(
            !refused
                .display_message()
                .to_lowercase()
                .contains("pool timed out"),
            "the driver's sentence reached the wire: {}",
            refused.display_message()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_saturated_pool_is_still_ready() {
        let schema = SqlSchema::mint().await.expect("a test schema");
        let pool = one_connection_pool(&schema, Duration::from_millis(250)).await;

        let held = pool
            .connector()
            .acquire()
            .await
            .expect("the one connection, which is now every connection");

        let readiness = fundamental_db::HealthCheck::health_check(&pool).await;
        drop(held);

        assert!(
            readiness.is_ok(),
            "a saturated pool reported itself unready, so a burst would de-register the \
             replicas serving it: {readiness:?}"
        );
    }
}
