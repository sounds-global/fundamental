//! Integration tests for the YCQL adapter.
//!
//! These tests require Docker.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use fundamental_core::Error;
use fundamental_core::model::CqlDb;
use fundamental_core::secrets::{Secret, SecretBytes, SecretJson, SecretString};
use fundamental_cql::migration::CqlIntrospector;
use fundamental_db::migration::{
    DatabaseSchema, DiffConfig, MigrationDatabase, ModelSchema, SchemaDiffer, SchemaIntrospector,
};
use fundamental_db::{
    Filter, FilterValue, KeyedRepository, KeyedRepositoryBatch, RecordKey, Repository,
    RepositoryAggregate, RepositoryBatch, RepositoryExt, RepositorySoftDelete,
    RepositoryTranslation, TlsMode,
};
use fundamental_macros::Model;
// The driver's `CqlValue` is already in scope below; the seam binds this crate's
// own. A genuine name clash is the one case the import rule allows renaming for.
use fundamental_cql::CqlValue as BoundValue;
use fundamental_cql::{
    BatchType, CircuitBreaker, CircuitBreakerConfig, CircuitState, Consistency, CounterOps,
    CqlBatch, CqlConfig, CqlConnections, CqlIdent, CqlLwt, CqlMetrics, CqlMetricsConfig, CqlPool,
    CqlSliceStore, CqlWriteTime, DEFAULT_DDL_TIMEOUT, DEFAULT_MAX_UNPAGED_RESULTS, Ddl,
    PagingConfig, PrometheusMetrics, RateLimit, RateLimitDecision, RetryConfig, ToRow,
    check_result_size, cql, create_db_span, ddl, record_outcome, with_retry,
};
use fundamental_sync::wire::SliceCursor;
use fundamental_sync::{CaptureTable, SyncMeta, SyncRecord};

use scylla::value::{CqlTimeuuid, CqlValue, Row};
/// Reads one counter cell, accepting either wire shape.
///
/// Cassandra sends `Native(Counter)`, YCQL `Native(BigInt)`, as `CounterOps` handles too.
fn counter_value(row: CqlValue) -> i64 {
    match row {
        CqlValue::Counter(value) => value.0,
        CqlValue::BigInt(value) => value,
        other => panic!("a counter column is neither a counter nor a bigint: {other:?}"),
    }
}

use fundamental_test_containers::cql::{CqlKeyspace, CqlNode};
use tokio::sync::OnceCell;

/// This binary's YCQL node.
async fn node() -> &'static CqlNode {
    CqlNode::shared().await.expect("a YCQL node")
}

/// Test fixture for YCQL integration tests.
struct CqlTestFixture {
    pool: CqlPool,
    keyspace: String,
    /// Held for the whole test, not read: the keyspace is removed when this drops.
    _owned: CqlKeyspace,
}

impl CqlTestFixture {
    async fn new() -> Self {
        let owned = CqlKeyspace::mint(node().await)
            .await
            .expect("a YCQL test keyspace");
        Self {
            pool: owned.pool().clone(),
            keyspace: owned.name().to_owned(),
            _owned: owned,
        }
    }

    async fn create_test_table(&self) {
        let cql = format!(
            "CREATE TABLE IF NOT EXISTS {}.events (
                tenant_id uuid,
                event_id timeuuid,
                event_type text,
                payload text,
                PRIMARY KEY (tenant_id, event_id)
            ) WITH CLUSTERING ORDER BY (event_id DESC)",
            self.keyspace
        );

        self.pool
            .session()
            .inner()
            .query_unpaged(cql, &[])
            .await
            .expect("Failed to create test table");
    }

    async fn create_counter_table(&self) {
        let cql = format!(
            "CREATE TABLE IF NOT EXISTS {}.rate_limits (
                key text,
                window_start bigint,
                count counter,
                PRIMARY KEY (key, window_start)
            )",
            self.keyspace
        );

        self.pool
            .session()
            .inner()
            .query_unpaged(cql, &[])
            .await
            .expect("Failed to create counter table");
    }
}

#[tokio::test]
async fn test_connection_and_health_check() {
    let fixture = CqlTestFixture::new().await;

    assert!(fixture.pool.health_check().await.is_ok());
}

#[tokio::test]
async fn test_keyspace_creation() {
    let fixture = CqlTestFixture::new().await;

    let result = fixture
        .pool
        .session()
        .inner()
        .query_unpaged(
            "SELECT keyspace_name FROM system_schema.keyspaces WHERE keyspace_name = ?",
            (&fixture.keyspace,),
        )
        .await
        .expect("Failed to query keyspaces");

    let rows = result.into_rows_result().expect("Failed to get rows");
    assert!(
        rows.rows::<(String,)>()
            .expect("Failed to parse rows")
            .count()
            > 0
    );
}

#[tokio::test]
async fn test_insert_and_select() {
    let fixture = CqlTestFixture::new().await;
    fixture.create_test_table().await;

    let tenant_id = uuid::Uuid::new_v4();
    let event_id = CqlTimeuuid::from(uuid::Uuid::now_v1(&[1, 2, 3, 4, 5, 6]));
    let event_type = "user.created";
    let payload = r#"{"user_id": "123"}"#;

    let insert_cql = format!(
        "INSERT INTO {}.events (tenant_id, event_id, event_type, payload) VALUES (?, ?, ?, ?)",
        fixture.keyspace
    );

    fixture
        .pool
        .session()
        .inner()
        .query_unpaged(insert_cql, (tenant_id, event_id, event_type, payload))
        .await
        .expect("Failed to insert event");

    let select_cql = format!(
        "SELECT event_type, payload FROM {}.events WHERE tenant_id = ? AND event_id = ?",
        fixture.keyspace
    );

    let result = fixture
        .pool
        .session()
        .inner()
        .query_unpaged(select_cql, (tenant_id, event_id))
        .await
        .expect("Failed to select event");

    let rows = result.into_rows_result().expect("Failed to get rows");
    let mut iter = rows
        .rows::<(String, String)>()
        .expect("Failed to parse rows");

    let (retrieved_type, retrieved_payload) = iter
        .next()
        .expect("No rows returned")
        .expect("Failed to parse row");
    assert_eq!(retrieved_type, event_type);
    assert_eq!(retrieved_payload, payload);
}

#[tokio::test]
async fn test_prepared_statement_caching() {
    let fixture = CqlTestFixture::new().await;
    fixture.create_test_table().await;

    let tenant_id = uuid::Uuid::new_v4();
    let insert_cql = format!(
        "INSERT INTO {}.events (tenant_id, event_id, event_type, payload) VALUES (?, ?, ?, ?)",
        fixture.keyspace
    );

    // Through the prepared-statement cache: `query_unpaged` would never populate it.
    for i in 0..5 {
        let event_id = CqlTimeuuid::from(uuid::Uuid::now_v1(&[1, 2, 3, 4, 5, i as u8]));
        let prepared = fixture
            .pool
            .cache()
            .get_or_prepare(insert_cql.as_str())
            .await
            .expect("Failed to prepare statement");
        fixture
            .pool
            .session()
            .inner()
            .execute_unpaged(&prepared, (tenant_id, event_id, "test", "payload"))
            .await
            .expect("Failed to insert");
    }

    let stats = fixture.pool.cache_stats().await;
    assert!(
        stats.hits > 0 || stats.size > 0,
        "Cache should have entries"
    );
}

#[tokio::test]
async fn test_lwt_insert_if_not_exists() {
    let fixture = CqlTestFixture::new().await;
    fixture.create_test_table().await;

    let tenant_id = uuid::Uuid::new_v4();
    let event_id = CqlTimeuuid::from(uuid::Uuid::now_v1(&[1, 2, 3, 4, 5, 6]));

    let lwt_cql = format!(
        "INSERT INTO {}.events (tenant_id, event_id, event_type, payload) VALUES (?, ?, ?, ?) IF NOT EXISTS",
        fixture.keyspace
    );

    let result = fixture
        .pool
        .session()
        .inner()
        .query_unpaged(lwt_cql.as_str(), (tenant_id, event_id, "test", "payload1"))
        .await
        .expect("Failed to execute LWT");

    assert!(lwt_applied(result), "First insert should be applied");

    let result = fixture
        .pool
        .session()
        .inner()
        .query_unpaged(lwt_cql.as_str(), (tenant_id, event_id, "test", "payload2"))
        .await
        .expect("Failed to execute LWT");

    assert!(!lwt_applied(result), "Second insert should not be applied");
}

/// Reads the `[applied]` flag from an LWT (`IF NOT EXISTS`) result.
///
/// A failed condition appends the conflicting row's columns, so the row is read dynamically.
fn lwt_applied(result: scylla::response::query_result::QueryResult) -> bool {
    let rows = result.into_rows_result().expect("Failed to get rows");
    let row = rows
        .rows::<Row>()
        .expect("Failed to parse rows")
        .next()
        .expect("No rows")
        .expect("Failed to parse");
    matches!(row.columns.first(), Some(Some(CqlValue::Boolean(applied))) if *applied)
}

#[tokio::test]
async fn test_counter_operations() {
    let fixture = CqlTestFixture::new().await;
    fixture.create_counter_table().await;

    let key = "rate_limit:user:123";
    let window_start: i64 = 1700000000;

    let update_cql = format!(
        "UPDATE {}.rate_limits SET count = count + ? WHERE key = ? AND window_start = ?",
        fixture.keyspace
    );

    fixture
        .pool
        .session()
        .inner()
        .query_unpaged(update_cql.as_str(), (5_i64, key, window_start))
        .await
        .expect("Failed to increment counter");

    let select_cql = format!(
        "SELECT count FROM {}.rate_limits WHERE key = ? AND window_start = ?",
        fixture.keyspace
    );

    let result = fixture
        .pool
        .session()
        .inner()
        .query_unpaged(select_cql.as_str(), (key, window_start))
        .await
        .expect("Failed to read counter");

    let rows = result.into_rows_result().expect("Failed to get rows");
    let mut iter = rows.rows::<(CqlValue,)>().expect("Failed to parse rows");
    let (counter,) = iter.next().expect("No rows").expect("Failed to parse");
    assert_eq!(counter_value(counter), 5);

    fixture
        .pool
        .session()
        .inner()
        .query_unpaged(update_cql.as_str(), (3_i64, key, window_start))
        .await
        .expect("Failed to increment counter again");

    let result = fixture
        .pool
        .session()
        .inner()
        .query_unpaged(select_cql.as_str(), (key, window_start))
        .await
        .expect("Failed to read counter");

    let rows = result.into_rows_result().expect("Failed to get rows");
    let mut iter = rows.rows::<(CqlValue,)>().expect("Failed to parse rows");
    let (counter,) = iter.next().expect("No rows").expect("Failed to parse");
    assert_eq!(counter_value(counter), 8);
}

#[tokio::test]
async fn test_batch_insert() {
    let fixture = CqlTestFixture::new().await;
    fixture.create_test_table().await;

    let tenant_id = uuid::Uuid::new_v4();

    let insert_cql = format!(
        "INSERT INTO {}.events (tenant_id, event_id, event_type, payload) VALUES (?, ?, ?, ?)",
        fixture.keyspace
    );

    let mut batch = scylla::statement::batch::Batch::default();
    let mut values = Vec::new();
    for i in 0..10 {
        batch.append_statement(insert_cql.as_str());
        let event_id = CqlTimeuuid::from(uuid::Uuid::now_v1(&[1, 2, 3, 4, 5, i as u8]));
        values.push((tenant_id, event_id, format!("event_{}", i), "payload"));
    }

    fixture
        .pool
        .session()
        .inner()
        .batch(&batch, &values)
        .await
        .expect("Failed to execute batch");

    let count_cql = format!(
        "SELECT COUNT(*) FROM {}.events WHERE tenant_id = ?",
        fixture.keyspace
    );

    let result = fixture
        .pool
        .session()
        .inner()
        .query_unpaged(count_cql.as_str(), (tenant_id,))
        .await
        .expect("Failed to count events");

    let rows = result.into_rows_result().expect("Failed to get rows");
    let mut iter = rows.rows::<(i64,)>().expect("Failed to parse rows");
    let (count,) = iter.next().expect("No rows").expect("Failed to parse");
    assert_eq!(count, 10);
}

#[tokio::test]
async fn test_metrics_collection() {
    let fixture = CqlTestFixture::new().await;
    fixture.create_test_table().await;

    let metrics = Arc::new(CqlMetrics::new(
        CqlMetricsConfig::new().slow_query_threshold(Duration::from_millis(1)),
    ));

    let tenant_id = uuid::Uuid::new_v4();
    let event_id = CqlTimeuuid::from(uuid::Uuid::now_v1(&[1, 2, 3, 4, 5, 6]));

    let insert_cql = format!(
        "INSERT INTO {}.events (tenant_id, event_id, event_type, payload) VALUES (?, ?, ?, ?)",
        fixture.keyspace
    );

    let metrics_clone = Arc::clone(&metrics);
    metrics_clone
        .timed("insert", "events", async {
            fixture
                .pool
                .session()
                .inner()
                .query_unpaged(
                    insert_cql.as_str(),
                    (tenant_id, event_id, "test", "payload"),
                )
                .await
                .map_err(|e| e.to_string())
        })
        .await
        .expect("Failed to insert with timing");

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.base.queries_total, 1);
    // May or may not be a slow query depending on timing
}

#[tokio::test]
async fn test_ttl_insert() {
    let fixture = CqlTestFixture::new().await;
    fixture.create_test_table().await;

    let tenant_id = uuid::Uuid::new_v4();
    let event_id = CqlTimeuuid::from(uuid::Uuid::now_v1(&[1, 2, 3, 4, 5, 6]));

    let insert_cql = format!(
        "INSERT INTO {}.events (tenant_id, event_id, event_type, payload) VALUES (?, ?, ?, ?) USING TTL 10",
        fixture.keyspace
    );

    fixture
        .pool
        .session()
        .inner()
        .query_unpaged(
            insert_cql.as_str(),
            (tenant_id, event_id, "test", "payload"),
        )
        .await
        .expect("Failed to insert with TTL");

    let select_cql = format!(
        "SELECT TTL(payload) FROM {}.events WHERE tenant_id = ? AND event_id = ?",
        fixture.keyspace
    );

    let result = fixture
        .pool
        .session()
        .inner()
        .query_unpaged(select_cql.as_str(), (tenant_id, event_id))
        .await
        .expect("Failed to check TTL");

    let rows = result.into_rows_result().expect("Failed to get rows");
    let mut iter = rows.rows::<(Option<i64>,)>().expect("Failed to parse rows");
    let (ttl,) = iter.next().expect("No rows").expect("Failed to parse");
    assert!(ttl.is_some() && ttl.unwrap() > 0 && ttl.unwrap() <= 10);
}

/// A derived YCQL model, so the repository, query-builder, `ToRow` and `FromRow` paths run.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(cql)]
struct Event {
    #[model(partition_key)]
    tenant_id: uuid::Uuid,
    #[model(clustering_key)]
    bucket: i64,
    payload: String,
}

/// The keyspace the model-path tests share, minted once per **process**.
///
/// Minted, not fixed: nextest runs each test in its own process, which would rebuild a fixed
/// one mid-query.
static MODEL_KEYSPACE: OnceCell<CqlKeyspace> = OnceCell::const_new();

/// Returns a fresh pool whose session is scoped to the model keyspace; tests here close pools.
async fn model_pool() -> CqlPool {
    let config = node().await.config(Some(model_keyspace_name().await));
    CqlPool::connect(&config)
        .await
        .expect("connect a pool scoped to the model keyspace")
}

/// The model keyspace's name, having made sure it exists and is current.
///
/// Dropped and rebuilt rather than `IF NOT EXISTS`, so a table from an older model never
/// survives on the reused node.
async fn model_keyspace_name() -> &'static str {
    MODEL_KEYSPACE
        .get_or_init(|| async {
            let owned = CqlKeyspace::mint(node().await)
                .await
                .expect("a model-tests keyspace");
            let session = owned.session();
            for cql in [
                "CREATE TABLE events \
                 (tenant_id uuid, bucket bigint, payload text, PRIMARY KEY (tenant_id, bucket))",
                "CREATE TABLE rate_limits \
                 (key text, window_start bigint, count counter, PRIMARY KEY (key, window_start))",
                "CREATE TABLE accounts \
                 (id uuid PRIMARY KEY, email text, api_token text, \
                  recovery_key text, profile text)",
            ] {
                session
                    .inner()
                    .query_unpaged(cql, &[])
                    .await
                    .expect("create a model-tests table");
            }
            // The soft-delete model's own derived DDL, split into single statements.
            for stmt in SoftEvent::create_table_sql().split(';') {
                let stmt = stmt.trim();
                if stmt.is_empty() {
                    continue;
                }
                session
                    .inner()
                    .query_unpaged(stmt, &[])
                    .await
                    .expect("create soft_events table");
            }
            owned
        })
        .await
        .name()
}

#[tokio::test]
async fn test_repository_crud_and_lwt() {
    let pool = model_pool().await;
    let repo = pool.repository::<Event>();

    let tenant = uuid::Uuid::new_v4();
    let ev = Event {
        tenant_id: tenant,
        bucket: 1,
        payload: "hello".into(),
    };

    // insert + read back via query builder (ToRow -> CQL -> FromRow round-trip).
    repo.insert(&ev).await.expect("insert");
    let got = pool
        .query::<Event>()
        .partition_key(fundamental_cql::CqlValue::uuid(tenant))
        .execute()
        .await
        .expect("query");
    assert_eq!(got, vec![ev.clone()]);

    // update (YCQL insert-as-upsert via repository.update).
    let updated = Event {
        payload: "world".into(),
        ..ev.clone()
    };
    repo.update(&updated).await.expect("update");
    let one = pool
        .query::<Event>()
        .partition_key(fundamental_cql::CqlValue::uuid(tenant))
        .clustering_eq("bucket", fundamental_cql::CqlValue::bigint(1))
        .execute_one()
        .await
        .expect("execute_one");
    assert_eq!(one.unwrap().payload, "world");

    // LWT: insert_if_not_exists is false for an existing row, true for a new one.
    assert!(!repo.insert_if_not_exists(&ev).await.expect("lwt existing"));
    let fresh = Event {
        bucket: 2,
        ..ev.clone()
    };
    assert!(repo.insert_if_not_exists(&fresh).await.expect("lwt new"));

    // delete by composite key (the abstract delete takes the key values).
    assert!(
        KeyedRepository::delete(&repo, &ev.to_key_values())
            .await
            .expect("delete")
    );

    // insert_batch (Unlogged) writes multiple rows.
    let batch: Vec<Event> = (10..15)
        .map(|b| Event {
            tenant_id: tenant,
            bucket: b,
            payload: format!("b{b}"),
        })
        .collect();
    repo.insert_batch(&batch, BatchType::Unlogged)
        .await
        .expect("insert_batch");
    let all = pool
        .query::<Event>()
        .partition_key(fundamental_cql::CqlValue::uuid(tenant))
        .execute()
        .await
        .expect("query all");
    // bucket 2 (lwt) + buckets 10..15 (batch) = 6 rows (bucket 1 was deleted).
    assert_eq!(all.len(), 6);
}

#[tokio::test]
async fn test_lwt_insert_if_not_exists_with_ttl() {
    let pool = model_pool().await;
    let repo = pool.repository::<Event>();
    let tenant = uuid::Uuid::new_v4();

    let first = Event {
        tenant_id: tenant,
        bucket: 1,
        payload: "once".into(),
    };

    // The first writer wins: the row did not exist, so the conditional insert is
    // applied and the row is written with the TTL.
    assert!(
        repo.insert_if_not_exists_with_ttl(&first, 300)
            .await
            .expect("lwt+ttl new")
    );

    // A second writer with the same key loses the LWT atomically: not applied, and
    // the stored row is untouched (still the first writer's payload).
    let second = Event {
        payload: "again".into(),
        ..first.clone()
    };
    assert!(
        !repo
            .insert_if_not_exists_with_ttl(&second, 300)
            .await
            .expect("lwt+ttl existing")
    );

    let stored = pool
        .query::<Event>()
        .partition_key(fundamental_cql::CqlValue::uuid(tenant))
        .clustering_eq("bucket", fundamental_cql::CqlValue::bigint(1))
        .execute_one()
        .await
        .expect("read back")
        .expect("row present");
    assert_eq!(stored.payload, "once", "the losing LWT must not overwrite");

    // The TTL landed together with the conditional insert.
    let ttl_rows = pool
        .session()
        .inner()
        .query_unpaged(
            "SELECT TTL(payload) FROM events WHERE tenant_id = ? AND bucket = ?",
            (tenant, 1_i64),
        )
        .await
        .expect("select ttl")
        .into_rows_result()
        .expect("rows");
    let mut iter = ttl_rows.rows::<(Option<i64>,)>().expect("parse ttl rows");
    let (ttl,) = iter.next().expect("one row").expect("parse ttl");
    let ttl = ttl.expect("a TTL is set");
    assert!(
        ttl > 0 && ttl <= 300,
        "expected a positive TTL <= 300, got {ttl}"
    );
}

#[tokio::test]
async fn test_query_builder_clustering_filters() {
    let pool = model_pool().await;
    let repo = pool.repository::<Event>();
    let tenant = uuid::Uuid::new_v4();
    for b in 0..10 {
        repo.insert(&Event {
            tenant_id: tenant,
            bucket: b,
            payload: format!("p{b}"),
        })
        .await
        .expect("insert");
    }

    // clustering range >= 5 with a limit.
    let rows = pool
        .query::<Event>()
        .partition_key(fundamental_cql::CqlValue::uuid(tenant))
        .clustering_gte("bucket", fundamental_cql::CqlValue::bigint(5))
        .limit(3)
        .execute()
        .await
        .expect("range query");
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|e| e.bucket >= 5));

    // exact clustering match.
    let one = pool
        .query::<Event>()
        .partition_key(fundamental_cql::CqlValue::uuid(tenant))
        .clustering_eq("bucket", fundamental_cql::CqlValue::bigint(7))
        .execute()
        .await
        .expect("eq query");
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].bucket, 7);
}

/// A consistency level reaches the statement, and **this engine never refuses one**.
///
/// YCQL accepts every level on a single node, so this asserts only that the setter is live.
#[tokio::test]
async fn test_query_consistency_is_applied() {
    let pool = model_pool().await;
    let repo = pool.repository::<Event>();
    let tenant = uuid::Uuid::new_v4();
    repo.insert(&Event {
        tenant_id: tenant,
        bucket: 1,
        payload: "x".into(),
    })
    .await
    .expect("insert");

    // CL=One is satisfiable on a single node.
    let ok = pool
        .query::<Event>()
        .partition_key(fundamental_cql::CqlValue::uuid(tenant))
        .consistency(Consistency::One)
        .execute()
        .await;
    assert!(ok.is_ok(), "CL=ONE should succeed: {ok:?}");

    // YCQL serves CL=TWO anyway; the day it refuses, the level became load-bearing and this
    // says so.
    let two = pool
        .query::<Event>()
        .partition_key(fundamental_cql::CqlValue::uuid(tenant))
        .consistency(Consistency::Two)
        .execute()
        .await;
    assert!(
        two.is_ok(),
        "YCQL accepts an unsatisfiable consistency level rather than refusing it: {two:?}"
    );
}

#[tokio::test]
async fn test_counter_ops() {
    let pool = model_pool().await;

    let counters = counter_ops(&pool).await;
    let key = format!("user:{}", uuid::Uuid::new_v4().simple());
    let window = 1_700_000_000_i64;

    let v1 = counters
        .increment_rate_limit("count", &key, window, 5)
        .await
        .expect("increment");
    assert_eq!(v1, 5);
    let v2 = counters
        .increment_rate_limit("count", &key, window, 3)
        .await
        .expect("increment again");
    assert_eq!(v2, 8);
    let read = counters
        .get_rate_limit_counter("count", &key, window)
        .await
        .expect("read");
    assert_eq!(read, 8);
}

#[tokio::test]
async fn test_rate_limit_allows_to_the_limit_then_sheds() {
    let pool = model_pool().await;
    let limiter = RateLimit::new(
        counter_ops(&pool).await,
        "count",
        Duration::from_secs(60),
        3,
    )
    .expect("a one-minute window of three");

    let key = format!("caller:{}", uuid::Uuid::new_v4().simple());
    // 10 s into a 60 s window aligned at 1_699_999_980, leaving 50 s.
    let now = 1_699_999_990_i64;

    for expected_remaining in [2_u64, 1, 0] {
        let decision = limiter.check(&key, now).await.expect("counted");
        assert_eq!(
            decision,
            RateLimitDecision::Allow {
                remaining: expected_remaining
            }
        );
    }

    let shed = limiter.check(&key, now).await.expect("counted");
    assert_eq!(
        shed,
        RateLimitDecision::Shed {
            retry_after: Duration::from_secs(50)
        },
        "the delay must be the window's real remaining time"
    );
    assert_eq!(shed.retry_after_secs(), Some(50));

    // The increment is unconditional, so hammering through a shed does not let a
    // caller stay under the limit — the window's count keeps climbing.
    let still_shed = limiter.check(&key, now).await.expect("counted");
    assert!(!still_shed.is_allowed());
}

#[tokio::test]
async fn test_rate_limit_new_window_resets_and_clears_the_previous() {
    let pool = model_pool().await;
    let limiter = RateLimit::new(
        counter_ops(&pool).await,
        "count",
        Duration::from_secs(60),
        2,
    )
    .expect("limiter");

    let key = format!("caller:{}", uuid::Uuid::new_v4().simple());
    let first_window_now = 1_700_000_010_i64;
    let previous_window_start = 1_700_000_010_i64.div_euclid(60) * 60;

    limiter.check(&key, first_window_now).await.expect("first");
    limiter.check(&key, first_window_now).await.expect("second");
    assert!(
        !limiter
            .check(&key, first_window_now)
            .await
            .expect("third")
            .is_allowed()
    );

    // One window later the allowance is whole again…
    let next_window_now = first_window_now + 60;
    assert_eq!(
        limiter.check(&key, next_window_now).await.expect("rolled"),
        RateLimitDecision::Allow { remaining: 1 }
    );

    // …and that first request of the new window reclaimed the old cell.
    let counters = counter_ops(&pool).await;
    let stale = counters
        .get_rate_limit_counter("count", &key, previous_window_start)
        .await
        .expect("read the previous window");
    assert_eq!(stale, 0, "the previous window's cell must be reclaimed");
}

#[tokio::test]
async fn test_write_time_makes_a_late_older_write_lose() {
    let pool = model_pool().await;
    let repository = pool.repository::<Event>();
    let tenant = uuid::Uuid::new_v4();

    let newer = Event {
        tenant_id: tenant,
        bucket: 1,
        payload: "newer".to_string(),
    };
    let older = Event {
        tenant_id: tenant,
        bucket: 1,
        payload: "older".to_string(),
    };

    // Relative to now: a server may refuse a write-time far ahead of its clock.
    let now_micros = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_micros() as i64;

    repository
        .insert_at(&newer, now_micros)
        .await
        .expect("the newer event");
    // Arrives second, but is stamped earlier — it must not win.
    repository
        .insert_at(&older, now_micros - 1_000_000)
        .await
        .expect("the redelivered older event");

    let rows = repository
        .query()
        .partition_key(fundamental_cql::CqlValue::uuid(tenant))
        .execute()
        .await
        .expect("read back");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].payload, "newer",
        "the event-time order must decide, not the arrival order"
    );
}

/// Builds a [`CounterOps`] over the shared model keyspace's `rate_limits` table.
async fn counter_ops(pool: &CqlPool) -> CounterOps {
    CounterOps::new(
        pool.session().clone(),
        pool.cache().clone(),
        model_keyspace_name().await,
        "rate_limits",
    )
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
    cb.force_close();
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn test_with_retry_helper() {
    let attempts = Arc::new(AtomicU32::new(0));
    let cfg = RetryConfig::new()
        .max_retries(5)
        .base_delay(Duration::from_millis(1));
    let result: Result<i32, _> = with_retry(cfg, || {
        let attempts = Arc::clone(&attempts);
        async move {
            let n = attempts.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                // RateLimited is classified retryable; Config would not be retried.
                Err(fundamental_cql::CqlError::RateLimited("transient".into()))
            } else {
                Ok(7)
            }
        }
    })
    .await;
    assert_eq!(result.expect("eventually ok"), 7);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_paging_result_size_guard() {
    let small = vec![0_u8; 10];
    assert!(check_result_size(&small, DEFAULT_MAX_UNPAGED_RESULTS).is_ok());
    let big = vec![0_u8; 5];
    assert!(check_result_size(&big, 4).is_err(), "over-limit must error");

    let cfg = PagingConfig::new(500).max_rows(1000);
    assert_eq!(cfg.page_size, 500);
}

#[tokio::test]
async fn test_metrics_prometheus_and_otel() {
    let prom = PrometheusMetrics::new("cql_test").expect("registry");
    prom.record_batch(3);
    prom.record_cache_hit();
    prom.record_lwt(true);
    let text = prom.encode().expect("encode");
    assert!(text.contains("cql_test"));

    let span = create_db_span("select", "events", "a_keyspace");
    record_outcome(&span, true, None);
}

#[tokio::test]
async fn test_pool_lifecycle() {
    let f = CqlTestFixture::new().await;

    let status = f.pool.status();
    // A multiplexing driver has no acquire, no idle set and no cheap connection count: `None`.
    assert_eq!(status.active, None);
    assert_eq!(status.idle, None);
    assert_eq!(status.min, None);
    assert_eq!(status.pending, None);
    assert_eq!(
        status.utilization(),
        None,
        "a utilisation derived from nothing must not be a number"
    );
    assert_eq!(f.pool.in_flight_count(), 0);

    let info = f.pool.session().cluster_info();
    assert!(info.total_nodes >= 1);

    f.pool
        .graceful_shutdown(Duration::from_secs(5))
        .await
        .expect("graceful shutdown");
    assert!(f.pool.is_shutting_down());
}

#[tokio::test]
async fn test_tls_config_is_applied() {
    let contact_point = node().await.contact_point();

    // Routing off, as everywhere against this node.
    let tls_required = CqlConfig::new([contact_point])
        .default_consistency(Consistency::One)
        .token_aware_routing(false)
        .tls_mode(TlsMode::Required);
    assert!(
        CqlPool::connect(&tls_required).await.is_err(),
        "TLS Required must fail against a non-TLS node (proves config.tls is wired)"
    );

    let tls_disabled = CqlConfig::new([contact_point])
        .default_consistency(Consistency::One)
        .token_aware_routing(false)
        .tls_mode(TlsMode::Disabled);
    assert!(
        CqlPool::connect(&tls_disabled).await.is_ok(),
        "plaintext connection should still succeed when TLS is disabled"
    );
}

/// Read back off the profile rather than timed: a timeout small enough to beat a local round
/// trip never fires, so a timing probe would pass either way.
#[tokio::test]
async fn test_request_timeout_is_applied() {
    let contact_point = node().await.contact_point();
    let chosen = Duration::from_secs(97);

    let config = CqlConfig::new([contact_point])
        .default_consistency(Consistency::One)
        .token_aware_routing(false)
        .tls_mode(TlsMode::Disabled)
        .request_timeout(chosen);
    let pool = CqlPool::connect(&config).await.expect("a connected pool");

    let applied = pool
        .session()
        .inner()
        .get_default_execution_profile_handle()
        .to_profile()
        .get_request_timeout();

    // Not merely "some timeout": the driver's own default is 30s, so an unwired field
    // would still answer with a plausible-looking `Some`.
    assert_eq!(
        applied,
        Some(chosen),
        "the session must carry the configured request timeout, not the driver's default"
    );
}

/// Read off the executor, for the reason the test above states.
#[tokio::test]
async fn test_ddl_timeout_reaches_the_executor() {
    let contact_point = node().await.contact_point();
    let chosen = Duration::from_secs(83);

    let config = CqlConfig::new([contact_point])
        .default_consistency(Consistency::One)
        .token_aware_routing(false)
        .tls_mode(TlsMode::Disabled)
        .ddl_timeout(chosen);
    let pool = CqlPool::connect(&config).await.expect("a connected pool");

    assert_eq!(
        pool.cql().ddl_timeout(),
        chosen,
        "a schema statement must run under the configured DDL deadline"
    );
    assert_ne!(
        chosen, DEFAULT_DDL_TIMEOUT,
        "the value under test must not be the default, or an unwired field would pass"
    );
}

/// A model bound to the named `reports` connection, not the default one.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(cql, domain = "reports")]
struct Report {
    #[model(partition_key)]
    id: uuid::Uuid,
    label: String,
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn test_domain_routes_to_named_connection() {
    // Two keyspaces standing in for two YCQL instances.
    let owned_default = CqlKeyspace::mint(node().await)
        .await
        .expect("default keyspace");
    let owned_reports = CqlKeyspace::mint(node().await)
        .await
        .expect("reports keyspace");
    let (ks_default, ks_reports) = (
        owned_default.name().to_owned(),
        owned_reports.name().to_owned(),
    );

    // The same `reports` table in each, so routing — not table existence — is what the
    // assertion detects.
    for owned in [&owned_default, &owned_reports] {
        owned
            .session()
            .inner()
            .query_unpaged(
                "CREATE TABLE reports (id uuid PRIMARY KEY, label text)",
                &[],
            )
            .await
            .expect("create table");
    }

    let boot = CqlPool::connect(&node().await.config(None))
        .await
        .expect("bootstrap connect");
    let pool_default = owned_default.pool().clone();
    let pool_reports = owned_reports.pool().clone();

    let mut named = HashMap::new();
    named.insert("reports".to_string(), pool_reports.clone());
    let conns = CqlConnections::new(Some(pool_default.clone()), named);

    // `Report` declares `domain = "reports"`, so its repository must route to the
    // reports pool — the row lands in ks_reports, not ks_default.
    let repo = conns.repository::<Report>().expect("route to reports");
    let id = uuid::Uuid::new_v4();
    repo.insert(&Report {
        id,
        label: "q1".into(),
    })
    .await
    .expect("insert via routed repo");

    let count = |ks: &str| {
        let cql = format!("SELECT id FROM {ks}.reports");
        async {
            boot.session()
                .inner()
                .query_unpaged(cql, &[])
                .await
                .expect("count query")
                .into_rows_result()
                .expect("rows")
                .rows::<(uuid::Uuid,)>()
                .expect("typed rows")
                .count()
        }
    };
    assert_eq!(
        count(&ks_reports).await,
        1,
        "row must land in the reports keyspace"
    );
    assert_eq!(
        count(&ks_default).await,
        0,
        "default keyspace must be untouched"
    );

    // Unknown domains are a clear error, not a silent fallback.
    assert!(conns.pool_for_domain("nonexistent").is_err());
}

/// A YCQL model with `Secret` fields of all three at-rest shapes: text, binary and structured.
#[derive(Clone, Debug, Model)]
#[model(cql, table = "accounts")]
struct Account {
    #[model(id, partition_key)]
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
    let pool = model_pool().await;

    let km: fundamental_encryption::SharedKeyManager = std::sync::Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[5u8; 32])
            .build()
            .expect("build key manager"),
    );
    // The repository carries the key manager; the model only ever holds plaintext.
    let repo = pool.repository::<Account>().with_key_manager(km.clone());

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

    // Every secret column and its at-rest key id.
    assert_eq!(
        <Account as fundamental_core::model::Model>::secret_columns(),
        &[
            ("api_token", "pii"),
            ("recovery_key", "pii"),
            ("profile", "pii"),
        ]
    );

    // At rest every secret column holds the ciphertext envelope — never the
    // plaintext, and never the `[REDACTED]` marker a naive `Serialize` would emit.
    let rows = pool
        .session()
        .inner()
        .query_unpaged(
            format!("SELECT api_token, recovery_key, profile FROM accounts WHERE id = {id}"),
            &[],
        )
        .await
        .expect("raw select")
        .into_rows_result()
        .expect("rows");
    let (api_token, recovery_key, profile): (String, String, String) = rows
        .rows::<(String, String, String)>()
        .expect("typed rows")
        .next()
        .expect("one row")
        .expect("decode");
    for stored in [&api_token, &recovery_key, &profile] {
        assert!(
            stored.contains("ciphertext"),
            "column must store the envelope, got: {stored}"
        );
        assert!(
            !stored.contains(plaintext) && !stored.contains("a1b2"),
            "plaintext must never be persisted at rest"
        );
        assert!(
            !stored.contains("[REDACTED]"),
            "the redaction marker must never reach the database"
        );
    }

    // Read-back decrypts each secret into its native type.
    let got = repo
        .query()
        .partition_key(fundamental_cql::CqlValue::uuid(id))
        .execute_one()
        .await
        .expect("query")
        .expect("present");
    assert_eq!(got.api_token.expose().as_str(), plaintext);
    assert_eq!(got.recovery_key.expose(), &key_bytes);
    assert_eq!(got.profile.expose(), &codes);
    // Debug stays redacted even after a successful decrypt.
    assert_eq!(format!("{:?}", got.api_token), "[REDACTED]");
    assert_eq!(format!("{:?}", got.recovery_key), "[REDACTED]");
    assert_eq!(format!("{:?}", got.profile), "[REDACTED]");
}

#[tokio::test]
async fn a_ciphertext_moved_to_another_row_refuses_to_decrypt() {
    let pool = model_pool().await;
    let km: fundamental_encryption::SharedKeyManager = std::sync::Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[5u8; 32])
            .build()
            .expect("build key manager"),
    );
    let repo = pool.repository::<Account>().with_key_manager(km);

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

    let session = pool.session().inner();
    let rows = session
        .query_unpaged(
            format!("SELECT api_token FROM accounts WHERE id = {owner}"),
            &[],
        )
        .await
        .expect("raw select")
        .into_rows_result()
        .expect("rows");
    let (owner_envelope,): (String,) = rows
        .rows::<(String,)>()
        .expect("typed rows")
        .next()
        .expect("one row")
        .expect("decode");
    session
        .query_unpaged(
            format!("UPDATE accounts SET api_token = ? WHERE id = {other}"),
            (owner_envelope,),
        )
        .await
        .expect("move the envelope");
    let moved = repo
        .query()
        .partition_key(fundamental_cql::CqlValue::uuid(other))
        .execute_one()
        .await
        .expect_err("a moved ciphertext must not decrypt");
    assert_refused_decrypt(&moved, "api_token");

    let owners = repo
        .query()
        .partition_key(fundamental_cql::CqlValue::uuid(owner))
        .execute_one()
        .await
        .expect("the owner's own row still opens")
        .expect("present");
    assert_eq!(owners.api_token.expose().as_str(), "sk-owner");
}

#[tokio::test]
async fn a_value_whose_bound_column_changed_refuses_to_decrypt() {
    let pool = model_pool().await;
    let km: fundamental_encryption::SharedKeyManager = std::sync::Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[5u8; 32])
            .build()
            .expect("build key manager"),
    );
    let repo = pool.repository::<Account>().with_key_manager(km);
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
    let intact = repo
        .query()
        .partition_key(fundamental_cql::CqlValue::uuid(id))
        .execute_one()
        .await
        .expect("an untouched row opens")
        .expect("present");
    assert_eq!(intact.api_token.expose().as_str(), "sk-live");

    pool.session()
        .inner()
        .query_unpaged(
            format!("UPDATE accounts SET email = 'edited@example.com' WHERE id = {id}"),
            &[],
        )
        .await
        .expect("edit the bound column");
    let edited = repo
        .query()
        .partition_key(fundamental_cql::CqlValue::uuid(id))
        .execute_one()
        .await
        .expect_err("a row whose bound column changed must not decrypt");
    assert_refused_decrypt(&edited, "api_token");
}

/// A read refused because a secret did not open under its row, naming the column.
fn assert_refused_decrypt(error: &fundamental_db::DatabaseError, column: &str) {
    let expected = format!("Row conversion error: decrypt secret '{column}' failed");
    assert!(
        matches!(error, fundamental_db::DatabaseError::Serialization { message } if message.starts_with(&expected)),
        "expected a refused decrypt of '{column}', got {error:?}"
    );
}

#[tokio::test]
async fn secret_insert_without_key_manager_is_a_typed_error() {
    let pool = model_pool().await;

    // A pool-built repository carries no key manager; persisting a Secret column
    // must fail with a typed error rather than panic or store plaintext.
    let repo = pool.repository::<Account>();
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

/// Attached at the registry, since a Vault-dynamic pool is replaced on every lease rotation.
#[tokio::test]
async fn a_registry_collector_records_queries_from_the_pools_it_hands_out() {
    let pool = model_pool().await;

    let prometheus = Arc::new(
        PrometheusMetrics::in_registry("registry_metrics", &prometheus::Registry::new())
            .expect("the metric set registers"),
    );
    let conns =
        CqlConnections::new(Some(pool.clone()), HashMap::new()).with_metrics(prometheus.clone());

    let registry_pool = conns.pool_for_domain("default").expect("pool for default");
    assert!(
        registry_pool.metrics().is_some(),
        "a pool taken out of a metrics-configured registry must carry the collector"
    );

    registry_pool
        .repository::<Event>()
        .insert(&Event {
            tenant_id: uuid::Uuid::new_v4(),
            bucket: 1,
            payload: "counted".to_string(),
        })
        .await
        .expect("insert through the registry pool");

    let scrape = prometheus.encode().expect("encode the scrape");
    assert!(
        scrape.contains("registry_metrics"),
        "the namespace must appear in the scrape: {scrape}"
    );
    assert!(
        scrape.lines().any(|line| {
            line.starts_with("registry_metrics_cql_queries_total{") && !line.ends_with(" 0")
        }),
        "the write must have been counted, or the collector is attached and inert: {scrape}"
    );
}

#[tokio::test]
async fn secret_round_trips_through_a_registry_pool() {
    let pool = model_pool().await;

    let km: fundamental_encryption::SharedKeyManager = std::sync::Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[7u8; 32])
            .build()
            .expect("build key manager"),
    );
    // The pool taken out of a registry with a key manager must carry it onto its repository.
    let conns =
        CqlConnections::new(Some(pool.clone()), HashMap::new()).with_key_manager(km.clone());
    let registry_pool = conns.pool_for_domain("default").expect("pool for default");
    assert!(
        registry_pool.key_manager().is_some(),
        "a pool taken out of a km-configured registry must carry the km"
    );

    let id = uuid::Uuid::new_v4();
    let token = "sk-live-registry-pool";
    registry_pool
        .repository::<Account>()
        .insert(&Account {
            id,
            email: "pool@example.com".to_string(),
            api_token: Secret::new(token.to_string()),
            recovery_key: Secret::new(vec![9u8, 8, 7]),
            profile: Secret::new(vec!["p".to_string()]),
        })
        .await
        .expect("pool repository insert encrypts the secret");

    // Ciphertext at rest, never plaintext.
    let rows = pool
        .session()
        .inner()
        .query_unpaged(
            format!("SELECT api_token FROM accounts WHERE id = {id}"),
            &[],
        )
        .await
        .expect("raw select")
        .into_rows_result()
        .expect("rows");
    let (api_token,): (String,) = rows
        .rows::<(String,)>()
        .expect("typed rows")
        .next()
        .expect("one row")
        .expect("decode");
    assert!(api_token.contains("ciphertext"), "must store the envelope");
    assert!(
        !api_token.contains(token),
        "plaintext must never be persisted"
    );

    // Decrypts back through the pool repository.
    let got = registry_pool
        .repository::<Account>()
        .query()
        .partition_key(fundamental_cql::CqlValue::uuid(id))
        .execute_one()
        .await
        .expect("query")
        .expect("present");
    assert_eq!(got.api_token.expose().as_str(), token);
}

/// A notice of one organisation, newest first, with a title in several languages: its
/// translations live in the organisation's partition, keyed by the notice's own key.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(cql, table = "localized_notices")]
struct LocalizedNotice {
    #[model(partition_key)]
    organisation_id: uuid::Uuid,
    #[model(clustering_key = "desc")]
    id: i64,
    title: fundamental_core::i18n::Localized<String>,
}

/// Creates the notice's table and its companion.
async fn create_notice_tables(pool: &CqlPool) {
    for cql in [
        LocalizedNotice::create_table_sql(),
        LocalizedNotice::translation_table_sql(),
    ] {
        pool.session()
            .inner()
            .query_unpaged(cql, &[])
            .await
            .expect("create the notice tables");
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one partition's whole story: write, read, search, delete, batch"
)]
async fn a_translation_lives_in_its_records_partition_and_goes_with_its_key() {
    use fundamental_core::i18n::{Locale, Localized};

    let pool = model_pool().await;
    create_notice_tables(&pool).await;
    // The companion read back is the one the model expects, so the drift check knows it.
    let live = CqlIntrospector::introspect(&pool)
        .await
        .expect("introspect the live schema");
    let mut actual = DatabaseSchema::for_database(MigrationDatabase::CqlDb);
    actual.add_table(live.tables["localized_notices_i18n"].clone());
    let mut expected = DatabaseSchema::for_database(MigrationDatabase::CqlDb);
    expected.add_table(
        <LocalizedNotice as ModelSchema<CqlDb>>::translation_schema().expect("a companion"),
    );
    let drift = SchemaDiffer::with_config(DiffConfig {
        ignore_column_order: true,
        ..Default::default()
    })
    .diff(&expected, &actual);
    assert!(
        drift.is_empty(),
        "the companion as the model defines it: {drift:?}"
    );

    let repo = pool.repository::<LocalizedNotice>();
    let organisation = uuid::Uuid::new_v4();
    let nl: Locale = "nl".parse().expect("a language");
    let notice = |id: i64, title: &str, dutch: &str| LocalizedNotice {
        organisation_id: organisation,
        id,
        title: Localized::new(title).with("nl", dutch),
    };

    // Two records in one partition, each with its own translation.
    repo.insert(&notice(1, "News", "Nieuws"))
        .await
        .expect("insert");
    repo.insert(&notice(2, "Update", "Bijwerking"))
        .await
        .expect("insert");
    let first = repo
        .find(&(organisation, 1))
        .await
        .expect("read")
        .expect("the first notice");
    assert_eq!(first.title.resolve(&nl), "Nieuws");
    assert_eq!(
        repo.find_ids_by_translation("title", "nl", "Bijwerking")
            .await
            .expect("search"),
        [(organisation, 2)],
        "the record's own key, typed"
    );

    // A delete by key takes the record's translations, and only its.
    assert!(repo.delete(&(organisation, 1)).await.expect("delete"));
    repo.insert(&LocalizedNotice {
        organisation_id: organisation,
        id: 1,
        title: Localized::new("Other"),
    })
    .await
    .expect("insert again");
    let again = repo
        .find(&(organisation, 1))
        .await
        .expect("read")
        .expect("the notice written again");
    assert_eq!(again.title.resolve(&nl), "Other", "no old translation");
    let second = repo
        .find(&(organisation, 2))
        .await
        .expect("read")
        .expect("the second notice");
    assert_eq!(second.title.resolve(&nl), "Bijwerking");

    // A batch carries its records' translations, and a batched delete clears them.
    repo.insert_many(&[notice(3, "Three", "Drie"), notice(4, "Four", "Vier")])
        .await
        .expect("insert a batch");
    let fourth = repo
        .find(&(organisation, 4))
        .await
        .expect("read")
        .expect("the fourth notice");
    assert_eq!(fourth.title.resolve(&nl), "Vier");
    repo.delete_many(&[(organisation, 3), (organisation, 4)])
        .await
        .expect("delete a batch");
    assert!(
        repo.find_ids_by_translation("title", "nl", "Vier")
            .await
            .expect("search")
            .is_empty(),
        "the batched delete took the translations"
    );
}

#[tokio::test]
async fn a_page_reads_its_own_translations_and_only_its() {
    use fundamental_core::i18n::{Locale, Localized};

    let pool = model_pool().await;
    create_notice_tables(&pool).await;
    let repo = pool.repository::<LocalizedNotice>();
    let (ours, theirs) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let notice = |organisation_id, id: i64| LocalizedNotice {
        organisation_id,
        id,
        title: Localized::new(format!("Notice {id}"))
            .with("nl", format!("Bericht {id}"))
            .with("de", format!("Mitteilung {id}")),
    };
    // Three notices of ours, newest first, and one of another organisation with the same id.
    for (organisation, id) in [(ours, 1), (ours, 2), (ours, 3), (theirs, 2)] {
        repo.insert(&notice(organisation, id))
            .await
            .expect("insert");
    }

    let page = pool
        .query::<LocalizedNotice>()
        .partition(ours)
        .limit(2)
        .execute()
        .await
        .expect("read a page");

    let (nl, de): (Locale, Locale) = ("nl".parse().expect("nl"), "de".parse().expect("de"));
    let said: Vec<(i64, String, String)> = page
        .iter()
        .map(|n| {
            (
                n.id,
                n.title.resolve(&nl).clone(),
                n.title.resolve(&de).clone(),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            (3, "Bericht 3".to_owned(), "Mitteilung 3".to_owned()),
            (2, "Bericht 2".to_owned(), "Mitteilung 2".to_owned()),
        ],
        "the newest two, each in both languages"
    );
    for read in &page {
        let one = repo
            .find(&(ours, read.id))
            .await
            .expect("read one")
            .expect("the notice");
        assert_eq!(read.title.resolve(&nl), one.title.resolve(&nl));
        assert_eq!(read.title.resolve(&de), one.title.resolve(&de));
    }
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(cql)]
struct LocalizedProduct {
    #[model(partition_key)]
    id: uuid::Uuid,
    name: fundamental_core::i18n::Localized<String>,
    price: i64,
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn test_cql_localized_round_trip() {
    use fundamental_core::i18n::{Locale, Localized};
    use fundamental_cql::CqlValue;

    let pool = model_pool().await;
    // The derive generates both tables' DDL; the session resolves the unqualified names.
    pool.session()
        .inner()
        .query_unpaged(LocalizedProduct::create_table_sql(), &[])
        .await
        .expect("create localizedproduct table");
    pool.session()
        .inner()
        .query_unpaged(LocalizedProduct::translation_table_sql(), &[])
        .await
        .expect("create companion table");

    let repo = pool.repository::<LocalizedProduct>();
    let id = uuid::Uuid::new_v4();
    let es: Locale = "es".parse().unwrap();
    let de: Locale = "de".parse().unwrap();
    let nl: Locale = "nl".parse().unwrap();

    let mut name = Localized::new("Wireless Headphones");
    name.set("es", "Auriculares inalámbricos")
        .set("nl", "Draadloze koptelefoon");
    let product = LocalizedProduct {
        id,
        name,
        price: 9999,
    };
    repo.insert(&product).await.expect("insert");

    // Read back via the query builder → translatable fields hydrated.
    let got = pool
        .query::<LocalizedProduct>()
        .partition_key(CqlValue::uuid(id))
        .execute()
        .await
        .expect("query")
        .into_iter()
        .next()
        .expect("one row");
    assert_eq!(got, product);
    assert_eq!(got.name.resolve(&es), "Auriculares inalámbricos");
    assert_eq!(got.name.resolve(&de), "Wireless Headphones"); // → source
    assert!(
        got.name
            .to_check(&fundamental_core::i18n::Languages::new(
                "en".parse().unwrap(),
                ["es".parse().unwrap()]
            ))
            .is_empty()
    );

    // Search the sidecar by an exact translated value → the records' own keys.
    let hits = repo
        .find_ids_by_translation("name", "es", "Auriculares inalámbricos")
        .await
        .expect("search");
    assert_eq!(hits, [(id,)]);

    // update rewrites companion rows: change es, drop nl, change source.
    let updated = LocalizedProduct {
        id,
        name: Localized::new("Wired Headphones").with("es", "Auriculares con cable"),
        price: 8999,
    };
    repo.update(&updated).await.expect("update");
    let got = pool
        .query::<LocalizedProduct>()
        .partition_key(CqlValue::uuid(id))
        .execute()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(got.name.resolve(&es), "Auriculares con cable");
    assert_eq!(
        got.name.resolve(&nl),
        "Wired Headphones",
        "nl override was dropped"
    );

    // A model-aware LWT delete removes the row and its companion rows.
    repo.delete_if_exists(&got).await.expect("delete");
    let remaining = pool
        .query::<LocalizedProduct>()
        .partition_key(CqlValue::uuid(id))
        .execute()
        .await
        .unwrap();
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn test_repository_batch_abstract() {
    let pool = model_pool().await;
    let repo = pool.repository::<Event>();
    let tenant = uuid::Uuid::new_v4();

    let key_of = |b: i64| {
        Event {
            tenant_id: tenant,
            bucket: b,
            payload: String::new(),
        }
        .to_key_values()
    };

    // insert_many via the abstract RepositoryBatch trait.
    let models: Vec<Event> = (0..5)
        .map(|b| Event {
            tenant_id: tenant,
            bucket: b,
            payload: format!("p{b}"),
        })
        .collect();
    assert_eq!(
        repo.insert_many(&models).await.expect("insert_many").len(),
        5
    );

    // find_many over present + absent composite keys; absent keys are skipped.
    let found = repo
        .find_many(&[key_of(0), key_of(999), key_of(3)])
        .await
        .expect("find_many");
    assert_eq!(found.len(), 2, "absent composite keys are skipped");

    // update_many (YCQL upsert) rewrites every payload.
    let updates: Vec<Event> = models
        .iter()
        .map(|e| Event {
            payload: "updated".into(),
            ..e.clone()
        })
        .collect();
    repo.update_many(&updates).await.expect("update_many");
    let all = pool
        .query::<Event>()
        .partition_key(fundamental_cql::CqlValue::uuid(tenant))
        .execute()
        .await
        .expect("query all");
    assert_eq!(all.len(), 5);
    assert!(all.iter().all(|e| e.payload == "updated"));

    // delete_many reports the requested key count and clears the rows.
    let del_keys: Vec<_> = (0..5).map(key_of).collect();
    assert_eq!(repo.delete_many(&del_keys).await.expect("delete_many"), 5);
    let remaining = pool
        .query::<Event>()
        .partition_key(fundamental_cql::CqlValue::uuid(tenant))
        .execute()
        .await
        .expect("query empty");
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn test_repository_aggregate_count_and_count_where() {
    let pool = model_pool().await;
    let repo = pool.repository::<Event>();
    let tenant = uuid::Uuid::new_v4();

    let n = 4_u64;
    for b in 0..n as i64 {
        repo.insert(&Event {
            tenant_id: tenant,
            bucket: b,
            payload: "x".into(),
        })
        .await
        .expect("insert");
    }

    // count() is a full-table COUNT(*) via the abstract trait. The keyspace is
    // shared across concurrent tests, so assert it includes at least our rows.
    let total = repo.count().await.expect("count");
    assert!(total >= n, "count() must include our {n} rows, got {total}");

    // An empty query counts the whole table; concurrent tests share it, so assert it includes ours.
    let empty = repo
        .count_where(&Repository::query(&repo))
        .await
        .expect("count_where with no filters");
    assert!(
        empty >= n,
        "empty count_where must count the whole table (>= our {n} rows), got {empty}"
    );

    // A partition-scoped predicate counts only this tenant's rows.
    let by_tenant = repo
        .count_where(&Repository::query(&repo).filter(Filter::eq(
            "tenant_id",
            FilterValue::Uuid(tenant.to_string()),
        )))
        .await
        .expect("partition-scoped count_where");
    assert_eq!(
        by_tenant, n,
        "partition-scoped count must equal our {n} rows"
    );

    // Narrowing on the clustering key counts the subset (buckets 2,3 → 2 rows).
    let subset = repo
        .count_where(
            &Repository::query(&repo)
                .filter(Filter::eq(
                    "tenant_id",
                    FilterValue::Uuid(tenant.to_string()),
                ))
                .filter(Filter::gte("bucket", 2_i64)),
        )
        .await
        .expect("clustering-narrowed count_where");
    assert_eq!(subset, 2, "buckets >= 2 must be 2 of our {n} rows");

    // A predicate on a non-key column is rejected rather than silently
    // full-scanning the cluster; the typed error carries CQL_011.
    let err = repo
        .count_where(&Repository::query(&repo).filter(Filter::eq("payload", "x")))
        .await
        .expect_err("non-key predicate must be rejected");
    assert_eq!(err.error_code(), "CQL_011");
}

#[tokio::test]
async fn test_repository_aggregate_count_where_soft_delete() {
    let pool = model_pool().await;
    let repo = pool.repository::<SoftEvent>();
    let tenant = uuid::Uuid::new_v4();

    let n = 3_i64;
    for b in 0..n {
        repo.insert(&SoftEvent {
            tenant_id: tenant,
            bucket: b,
            payload: "x".into(),
        })
        .await
        .expect("insert");
    }

    // Rebuilt per call: `count_where` consumes the builder.
    let pred = || {
        Repository::query(&repo).filter(Filter::eq(
            "tenant_id",
            FilterValue::Uuid(tenant.to_string()),
        ))
    };

    assert_eq!(repo.count_where(&pred()).await.expect("count live"), 3);

    // Soft-deleting one excludes it.
    let key = SoftEvent {
        tenant_id: tenant,
        bucket: 1,
        payload: String::new(),
    }
    .to_key_values();
    assert!(repo.soft_delete(&key).await.expect("soft_delete"));
    assert_eq!(
        repo.count_where(&pred()).await.expect("count after delete"),
        2,
        "soft-deleted row must be excluded"
    );

    // Restoring it brings it back into the count.
    assert!(repo.restore(&key).await.expect("restore"));
    assert_eq!(
        repo.count_where(&pred())
            .await
            .expect("count after restore"),
        3,
        "restored row must be counted again"
    );
}

#[tokio::test]
async fn test_repository_ext_cql() {
    let pool = model_pool().await;
    let repo = pool.repository::<Event>();
    let tenant = uuid::Uuid::new_v4();

    let key_of = |b: i64| {
        Event {
            tenant_id: tenant,
            bucket: b,
            payload: String::new(),
        }
        .to_key_values()
    };

    // find_or_create creates on a miss (LWT insert; the factory runs).
    let created = repo
        .find_or_create(&key_of(1), || Event {
            tenant_id: tenant,
            bucket: 1,
            payload: "created".into(),
        })
        .await
        .expect("find_or_create create");
    assert_eq!(created.payload, "created");

    // A hit returns the stored row; the factory must NOT run.
    let again = repo
        .find_or_create(&key_of(1), || {
            panic!("factory must not run when the row exists")
        })
        .await
        .expect("find_or_create hit");
    assert_eq!(again, created);

    // The abstract trait's `update_if_exists`, not the inherent `CqlLwt` one.
    let updated = RepositoryExt::update_if_exists(
        &repo,
        &Event {
            tenant_id: tenant,
            bucket: 1,
            payload: "updated".into(),
        },
    )
    .await
    .expect("update_if_exists present");
    assert_eq!(updated.expect("some").payload, "updated");

    // update_if_exists: absent row → None, and no row materializes.
    let missing = RepositoryExt::update_if_exists(
        &repo,
        &Event {
            tenant_id: tenant,
            bucket: 7,
            payload: "ghost".into(),
        },
    )
    .await
    .expect("update_if_exists absent");
    assert!(missing.is_none());
    assert!(repo.find(&key_of(7)).await.expect("find absent").is_none());
}

/// A soft-deletable YCQL model, its table made in [`model_pool`]'s bootstrap.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(cql, table = "soft_events", soft_delete)]
struct SoftEvent {
    #[model(partition_key)]
    tenant_id: uuid::Uuid,
    #[model(clustering_key)]
    bucket: i64,
    payload: String,
}

/// Independent raw-driver oracle: is `deleted_at` non-null for the given key?
async fn soft_event_deleted_at_set(pool: &CqlPool, tenant: uuid::Uuid, bucket: i64) -> bool {
    let result = pool
        .session()
        .inner()
        .query_unpaged(
            "SELECT deleted_at FROM soft_events WHERE tenant_id = ? AND bucket = ?",
            (tenant, bucket),
        )
        .await
        .expect("oracle query");
    let rows = result.into_rows_result().expect("oracle rows");
    let mut iter = rows.rows::<Row>().expect("oracle typed rows");
    match iter.next() {
        Some(Ok(row)) => matches!(row.columns.first(), Some(Some(_))),
        _ => false,
    }
}

#[tokio::test]
async fn test_repository_soft_delete_cql() {
    let pool = model_pool().await;
    let repo = pool.repository::<SoftEvent>();
    let tenant = uuid::Uuid::new_v4();

    let key = SoftEvent {
        tenant_id: tenant,
        bucket: 1,
        payload: String::new(),
    }
    .to_key_values();
    repo.insert(&SoftEvent {
        tenant_id: tenant,
        bucket: 1,
        payload: "hello".into(),
    })
    .await
    .expect("insert");

    // soft_delete (LWT) hides the row from default reads; find_with_deleted sees it.
    assert!(repo.soft_delete(&key).await.expect("soft_delete"));
    assert!(repo.find(&key).await.expect("find").is_none());
    assert!(
        repo.find_with_deleted(&key)
            .await
            .expect("find_with_deleted")
            .is_some()
    );
    assert!(
        soft_event_deleted_at_set(&pool, tenant, 1).await,
        "deleted_at stamped after soft_delete"
    );

    // restore makes it visible again and clears deleted_at.
    assert!(repo.restore(&key).await.expect("restore"));
    assert!(repo.find(&key).await.expect("find after restore").is_some());
    assert!(
        !soft_event_deleted_at_set(&pool, tenant, 1).await,
        "deleted_at cleared after restore"
    );

    // purge hard-deletes it, even from find_with_deleted.
    assert!(repo.purge(&key).await.expect("purge"));
    assert!(
        repo.find_with_deleted(&key)
            .await
            .expect("find_with_deleted after purge")
            .is_none()
    );
}

#[tokio::test]
async fn test_find_required_not_found_cql() {
    let pool = model_pool().await;
    let repo = pool.repository::<Event>();
    let tenant = uuid::Uuid::new_v4();

    let present = Event {
        tenant_id: tenant,
        bucket: 1,
        payload: "x".into(),
    };
    repo.insert(&present).await.expect("insert");
    assert_eq!(
        repo.find_required(&present.to_key_values())
            .await
            .expect("present"),
        present
    );

    // A missing composite key is a typed NotFound with the stable code + status.
    let missing_key = Event {
        tenant_id: tenant,
        bucket: 999,
        payload: String::new(),
    }
    .to_key_values();
    let err = repo
        .find_required(&missing_key)
        .await
        .expect_err("missing must error");
    assert_eq!(err.error_code(), "DB_004");
    assert_eq!(err.http_status_code(), 404);
}

#[derive(Clone, Debug, Model)]
#[model(cql, table = "widgets")]
struct Widget {
    #[model(partition_key)]
    tenant: String,
    #[model(clustering_key = "desc")]
    seq: i64,
    name: String,
}

/// A table from a model's own `CREATE TABLE`, read back by the introspector, diffs to nothing.
#[tokio::test]
async fn introspected_schema_matches_the_model() {
    let fixture = CqlTestFixture::new().await;
    let session = fixture.pool.session();
    session
        .inner()
        .use_keyspace(fixture.keyspace.clone(), false)
        .await
        .expect("select the test keyspace");

    fixture
        .pool
        .execute_ddl(&Ddl::interpolated(Widget::create_table_sql().to_owned()))
        .await
        .expect("create widgets table");

    let actual = CqlIntrospector::introspect(&fixture.pool)
        .await
        .expect("introspect the live schema");

    let mut expected = DatabaseSchema::for_database(MigrationDatabase::CqlDb);
    expected.add_table(<Widget as ModelSchema<CqlDb>>::table_schema());

    let differ = SchemaDiffer::with_config(DiffConfig {
        ignore_column_order: true,
        ..Default::default()
    });
    let changes = differ.diff(&expected, &actual);
    assert!(
        changes.is_empty(),
        "model and freshly-created CQL table must match, found: {changes:?}"
    );
}

/// A sync record for `owner`'s note; the owner is a key component, not a payload column.
fn note_record(id: &str, owner: &str, body: &str) -> SyncRecord {
    let mut payload = std::collections::BTreeMap::new();
    payload.insert("body".to_string(), body.to_string());
    SyncRecord::new(
        RecordKey::composite(owner, [id]),
        SyncMeta::new(1, 1),
        payload,
    )
}

/// `notes` is `PRIMARY KEY ((owner_id), id)`, so this is its key — partition first.
const NOTE_KEY: [&str; 2] = ["owner_id", "id"];
/// …and this is the partition half of it, which is what the outbox partitions by.
const NOTE_PARTITION: [&str; 1] = ["owner_id"];

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn slice_store_round_trips_through_the_outbox() {
    let fixture = CqlTestFixture::new().await;
    let session = fixture.pool.session().inner();

    // A per-owner base table: the owner column is the partition key.
    session
        .query_unpaged(
            format!(
                "CREATE TABLE {}.notes (owner_id text, id text, body text, \
                 PRIMARY KEY ((owner_id), id))",
                fixture.keyspace
            ),
            &[],
        )
        .await
        .expect("create base table");

    let store = CqlSliceStore::new(
        &fixture.pool,
        &fixture.keyspace,
        [CaptureTable::new(
            "notes",
            NOTE_KEY,
            NOTE_PARTITION,
            ["body"],
            "owner_id",
        )],
    );
    store.create_outbox_table().await.expect("create outbox");

    // A high-water mark just before the writes, within the retention window.
    let mark = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        - 1000;

    let cols = ["body"];
    for (id, body) in [("n1", "one"), ("n2", "two"), ("n3", "three")] {
        store
            .upsert("notes", &cols, &NOTE_KEY, &note_record(id, "alice", body))
            .await
            .expect("write alice note");
    }
    store
        .upsert(
            "notes",
            &cols,
            &NOTE_KEY,
            &note_record("b1", "bob", "bob-note"),
        )
        .await
        .expect("write bob note");

    let alice = [Filter::eq("owner_id", "alice")];

    // Paged read: two pages of size 2 cover all three of alice's notes, in order,
    // each carrying its payload — and never bob's.
    let page1 = store
        .changed_page("notes", &NOTE_KEY, &alice, mark, None, 2)
        .await
        .expect("first page");
    assert_eq!(page1.len(), 2, "a full first page");
    let last = page1.last().unwrap();
    let cursor = SliceCursor::new(last.meta.updated_at, last.id.clone());
    let page2 = store
        .changed_page("notes", &NOTE_KEY, &alice, mark, Some(&cursor), 2)
        .await
        .expect("second page");

    let mut ids: Vec<String> = page1
        .iter()
        .chain(&page2)
        .map(|r| r.id.to_string())
        .collect();
    ids.sort();
    assert_eq!(
        ids,
        vec!["(alice, n1)", "(alice, n2)", "(alice, n3)"],
        "every note exactly once across pages, each carrying its whole key"
    );
    assert!(
        page1
            .iter()
            .chain(&page2)
            .all(|r| r.id.components().next() == Some("alice")),
        "no cross-owner rows — read from the key, which is what scopes them"
    );
    assert_eq!(page1[0].payload["body"], "one");

    // Owner isolation: bob sees only his own.
    let bob = store
        .changed_page(
            "notes",
            &NOTE_KEY,
            &[Filter::eq("owner_id", "bob")],
            mark,
            None,
            10,
        )
        .await
        .expect("bob page");
    let bob_ids: Vec<String> = bob.iter().map(|r| r.id.to_string()).collect();
    assert_eq!(bob_ids, vec!["(bob, b1)"]);

    // A tombstone deletes the base row and records the delete in the outbox.
    let mut tombstone = note_record("n1", "alice", "one");
    tombstone.meta = tombstone.meta.tombstoned();
    store
        .upsert("notes", &cols, &NOTE_KEY, &tombstone)
        .await
        .expect("write tombstone");

    let after_delete = store
        .changed_page("notes", &NOTE_KEY, &alice, mark, None, 10)
        .await
        .expect("read after delete");
    assert!(
        after_delete
            .iter()
            .any(|r| r.id == RecordKey::composite("alice", ["n1"]) && r.meta.tombstone),
        "the delete propagates as a tombstone"
    );
    let base_rows = session
        .query_unpaged(
            format!(
                "SELECT id FROM {}.notes WHERE owner_id = 'alice' AND id = 'n1'",
                fixture.keyspace
            ),
            &[],
        )
        .await
        .expect("read base row")
        .into_rows_result()
        .expect("rows")
        .rows::<(String,)>()
        .expect("typed")
        .count();
    assert_eq!(base_rows, 0, "the base row is gone after a tombstone");

    // A full resync (since = 0) is refused — the outbox only retains a TTL window.
    let err = store
        .changed_page("notes", &NOTE_KEY, &alice, 0, None, 10)
        .await
        .expect_err("since = 0 is unsupported");
    assert!(matches!(err, fundamental_cql::CqlError::InvalidQuery(_)));
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn the_outbox_partitions_by_a_zone_led_key_and_refuses_an_unpinned_zone() {
    use fundamental_sync::wire::SliceCursor;

    let fixture = CqlTestFixture::new().await;
    let session = fixture.pool.session().inner();
    session
        .query_unpaged(
            format!(
                "CREATE TABLE {}.zoned (zone text, owner_id text, row_id text, body text, \
                 PRIMARY KEY ((zone, owner_id), row_id))",
                fixture.keyspace
            ),
            &[],
        )
        .await
        .expect("create zoned table");

    const KEY: [&str; 3] = ["zone", "owner_id", "row_id"];
    const PARTITION: [&str; 2] = ["zone", "owner_id"];

    let store = CqlSliceStore::new(
        &fixture.pool,
        &fixture.keyspace,
        [CaptureTable::new(
            "zoned",
            KEY,
            PARTITION,
            ["body"],
            "owner_id",
        )],
    );
    store.create_outbox_table().await.expect("create outbox");

    let mark = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as i64
        - 1000;

    let row = |zone: &str, owner: &str, row_id: &str, body: &str| {
        let mut payload = std::collections::BTreeMap::new();
        payload.insert("body".to_string(), body.to_string());
        SyncRecord::new(
            RecordKey::composite(zone, [owner, row_id]),
            SyncMeta::new(1, 1),
            payload,
        )
    };

    let cols = ["body"];
    for (zone, owner, row_id, body) in [
        ("eu", "alice", "r1", "eu-one"),
        ("eu", "alice", "r2", "eu-two"),
        // Same owner id, different zone: a different partition entirely.
        ("jp", "alice", "r1", "jp-one"),
    ] {
        store
            .upsert("zoned", &cols, &KEY, &row(zone, owner, row_id, body))
            .await
            .expect("write zoned row");
    }

    // Pinning both partition columns reads exactly that zone's partition.
    let eu = [Filter::eq("zone", "eu"), Filter::eq("owner_id", "alice")];
    let page = store
        .changed_page("zoned", &KEY, &eu, mark, None, 10)
        .await
        .expect("eu page");
    let mut ids: Vec<String> = page.iter().map(|r| r.id.to_string()).collect();
    ids.sort();
    assert_eq!(
        ids,
        vec!["(eu, alice, r1)", "(eu, alice, r2)"],
        "the whole key round-trips, and the same owner's `jp` row is a different \
         partition rather than a filtered-out row"
    );
    assert_eq!(page[0].payload["body"], "eu-one");

    // Paging inside the partition resumes on the clustering half.
    let first = store
        .changed_page("zoned", &KEY, &eu, mark, None, 1)
        .await
        .expect("first page");
    assert_eq!(first.len(), 1);
    let cursor = SliceCursor::new(first[0].meta.updated_at, first[0].id.clone());
    let second = store
        .changed_page("zoned", &KEY, &eu, mark, Some(&cursor), 10)
        .await
        .expect("second page");
    assert_eq!(
        second.iter().map(|r| r.id.to_string()).collect::<Vec<_>>(),
        vec!["(eu, alice, r2)"],
        "the cursor resumes after the first row, in its own partition"
    );

    // The other zone is reached only by naming it.
    let jp = [Filter::eq("zone", "jp"), Filter::eq("owner_id", "alice")];
    let jp_page = store
        .changed_page("zoned", &KEY, &jp, mark, None, 10)
        .await
        .expect("jp page");
    assert_eq!(
        jp_page.iter().map(|r| r.id.to_string()).collect::<Vec<_>>(),
        vec!["(jp, alice, r1)"]
    );

    // Serving it would scan every zone's partition.
    let unpinned = [Filter::eq("owner_id", "alice")];
    let err = store
        .changed_page("zoned", &KEY, &unpinned, mark, None, 10)
        .await
        .expect_err("an unpinned zone must be refused");
    assert!(
        err.to_string().contains("zone"),
        "the refusal names the column that was not pinned: {err}"
    );
}

// ── The CQL seam ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_cql_seam_writes_and_reads_through_the_pool() {
    let fixture = CqlTestFixture::new().await;
    let table = CqlIdent::qualified(&fixture.keyspace, "seam_rows").expect("a valid name");

    fixture
        .pool
        .execute_ddl(&ddl!(
            "CREATE TABLE IF NOT EXISTS {} (owner text, id text, PRIMARY KEY (owner, id))",
            table
        ))
        .await
        .expect("the schema statement runs");

    for id in ["a", "b", "c"] {
        fixture
            .pool
            .execute(
                &cql!("INSERT INTO {} (owner, id) VALUES (?, ?)", table)
                    .bind(BoundValue::text("acme"))
                    .bind(BoundValue::text(id)),
            )
            .await
            .expect("the write runs");
    }

    let read = cql!("SELECT id FROM {} WHERE owner = ?", table)
        .bind(BoundValue::text("acme"))
        .idempotent();

    let mut ids: Vec<String> = fixture
        .pool
        .fetch_all::<(String,)>(&read)
        .await
        .expect("the read runs")
        .into_iter()
        .map(|(id,)| id)
        .collect();
    ids.sort();
    assert_eq!(ids, vec!["a", "b", "c"]);

    let one = fixture
        .pool
        .fetch_one::<(String,)>(
            &cql!("SELECT id FROM {} WHERE owner = ? AND id = ?", table)
                .bind(BoundValue::text("acme"))
                .bind(BoundValue::text("b")),
        )
        .await
        .expect("exactly one row");
    assert_eq!(one.0, "b");

    let missing = fixture
        .pool
        .fetch_optional::<(String,)>(
            &cql!("SELECT id FROM {} WHERE owner = ? AND id = ?", table)
                .bind(BoundValue::text("acme"))
                .bind(BoundValue::text("nope")),
        )
        .await
        .expect("a miss is not an error");
    assert!(missing.is_none());
}

#[tokio::test]
async fn a_bounded_read_refuses_rather_than_materialising_everything() {
    let fixture = CqlTestFixture::new().await;
    let table = CqlIdent::qualified(&fixture.keyspace, "bounded_rows").expect("a valid name");

    fixture
        .pool
        .execute_ddl(&ddl!(
            "CREATE TABLE IF NOT EXISTS {} (owner text, id int, PRIMARY KEY (owner, id))",
            table
        ))
        .await
        .expect("the schema statement runs");

    for id in 0..25i32 {
        fixture
            .pool
            .execute(
                &cql!("INSERT INTO {} (owner, id) VALUES (?, ?)", table)
                    .bind(BoundValue::text("acme"))
                    .bind(BoundValue::Int(id)),
            )
            .await
            .expect("the write runs");
    }

    let read = cql!("SELECT id FROM {} WHERE owner = ?", table)
        .bind(BoundValue::text("acme"))
        .page_size(5)
        .idempotent();

    let err = fixture
        .pool
        .fetch_paged::<(i32,)>(&read, 10)
        .await
        .expect_err("25 rows must not come back under a bound of 10");
    assert_eq!(err.error_code(), "CQL_011");
    assert!(
        err.to_string().contains("exceeds 10 rows"),
        "the refusal names the bound it hit: {err}"
    );

    let all = fixture
        .pool
        .fetch_paged::<(i32,)>(&read, 100)
        .await
        .expect("a bound above the row count reads them all");
    assert_eq!(
        all.len(),
        25,
        "paging across a page size of 5 loses nothing"
    );
}

/// Nothing in the returned rows says whether a statement was prepared; the cache's counters do.
#[tokio::test]
async fn the_seam_prepares_its_statements() {
    let fixture = CqlTestFixture::new().await;
    let table = CqlIdent::qualified(&fixture.keyspace, "prepared_rows").expect("a valid name");

    fixture
        .pool
        .execute_ddl(&ddl!(
            "CREATE TABLE IF NOT EXISTS {} (owner text, id text, PRIMARY KEY (owner, id))",
            table
        ))
        .await
        .expect("the schema statement runs");

    fixture.pool.clear_cache().await;
    let before = fixture.pool.cache_stats().await;

    let read = cql!("SELECT id FROM {} WHERE owner = ?", table)
        .bind(BoundValue::text("acme"))
        .idempotent();
    for _ in 0..3 {
        fixture
            .pool
            .fetch_all::<(String,)>(&read)
            .await
            .expect("the read runs");
    }

    let after = fixture.pool.cache_stats().await;
    assert_eq!(
        after.size,
        before.size + 1,
        "the statement was prepared once and cached"
    );
    assert!(
        after.hits >= before.hits + 2,
        "the two repeats reused the cached statement rather than re-preparing: {after:?}"
    );
}

/// `ALL` is satisfiable on a single node, so this asserts the level is carried, not dropped.
#[tokio::test]
async fn a_statements_consistency_reaches_the_driver() {
    let fixture = CqlTestFixture::new().await;
    let table = CqlIdent::qualified(&fixture.keyspace, "consistency_rows").expect("a valid name");

    fixture
        .pool
        .execute_ddl(&ddl!(
            "CREATE TABLE IF NOT EXISTS {} (owner text, id text, PRIMARY KEY (owner, id))",
            table
        ))
        .await
        .expect("the schema statement runs");

    fixture
        .pool
        .execute(
            &cql!("INSERT INTO {} (owner, id) VALUES (?, ?)", table)
                .bind(BoundValue::text("acme"))
                .bind(BoundValue::text("only"))
                .consistency(Consistency::All),
        )
        .await
        .expect("a write at ALL is satisfiable on one node");

    let rows = fixture
        .pool
        .fetch_all::<(String,)>(
            &cql!("SELECT id FROM {} WHERE owner = ?", table)
                .bind(BoundValue::text("acme"))
                .consistency(Consistency::All)
                .idempotent(),
        )
        .await
        .expect("a read at ALL is satisfiable on one node");
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn the_seam_and_the_counters_record_their_timings() {
    let fixture = CqlTestFixture::new().await;
    let metrics = Arc::new(CqlMetrics::new(CqlMetricsConfig::default()));
    let pool = fixture.pool.clone().with_metrics(metrics.clone());
    let table = CqlIdent::qualified(&fixture.keyspace, "measured_rows").expect("a valid name");

    pool.execute_ddl(&ddl!(
        "CREATE TABLE IF NOT EXISTS {} (owner text, id text, PRIMARY KEY (owner, id))",
        table
    ))
    .await
    .expect("the schema statement runs");
    pool.execute(
        &cql!("INSERT INTO {} (owner, id) VALUES (?, ?)", table)
            .bind(BoundValue::text("acme"))
            .bind(BoundValue::text("one")),
    )
    .await
    .expect("the write runs");
    pool.fetch_all::<(String,)>(
        &cql!("SELECT id FROM {} WHERE owner = ?", table)
            .bind(BoundValue::text("acme"))
            .idempotent(),
    )
    .await
    .expect("the read runs");

    assert!(
        metrics.snapshot().base.queries_total >= 3,
        "the ddl, the write and the read were each measured: {:?}",
        metrics.snapshot()
    );

    fixture.create_counter_table().await;
    let measured_before = metrics.snapshot().base.queries_total;
    pool.counters("rate_limits")
        .expect("a plain table name")
        .increment_rate_limit("count", "seam-key", 0, 1)
        .await
        .expect("the counter increments");
    assert!(
        metrics.snapshot().base.queries_total > measured_before,
        "counters built from the pool inherit its collector"
    );

    // And the identifier guard holds on the way in.
    assert!(pool.counters("rate_limits; DROP KEYSPACE x; --").is_err());
}

// ── Graceful shutdown ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn graceful_shutdown_with_nothing_in_flight_succeeds_and_is_observable() {
    let fixture = CqlTestFixture::new().await;
    let pool = &fixture.pool;

    assert!(
        !pool.is_shutting_down(),
        "a fresh pool is not shutting down"
    );
    assert_eq!(pool.in_flight_count(), 0, "nothing has been issued yet");

    pool.graceful_shutdown(Duration::from_secs(5))
        .await
        .expect("nothing is in flight, so there is nothing to wait for");

    // The flag is the half that matters to a caller: it is what tells the rest of the
    // process to stop handing this pool new work.
    assert!(
        pool.is_shutting_down(),
        "shutdown must be observable after it is asked for, not only after it returns"
    );
}

#[tokio::test]
async fn graceful_shutdown_waits_for_an_operation_still_in_flight() {
    let fixture = CqlTestFixture::new().await;
    let pool = fixture.pool.clone();

    let guard = pool.operation_guard();
    assert_eq!(pool.in_flight_count(), 1, "the guard is the in-flight one");

    // Dropped from another task while the shutdown below is already polling, which is the
    // ordering the contract is about: the release happens *during* the wait, not before it.
    let releasing = pool.clone();
    let release = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(guard);
        // Read through the pool so the assertion is about what a caller can observe.
        releasing.in_flight_count()
    });

    // Not a timing assertion: the budget is reached only if the release never happens.
    pool.graceful_shutdown(Duration::from_secs(30))
        .await
        .expect("the in-flight operation was released, so shutdown completes");

    assert_eq!(
        release.await.expect("the releasing task did not panic"),
        0,
        "the guard's drop is what decremented the count"
    );
    assert_eq!(pool.in_flight_count(), 0);
}

#[tokio::test]
async fn graceful_shutdown_reports_what_was_still_in_flight_when_it_ran_out_of_time() {
    let fixture = CqlTestFixture::new().await;
    let pool = &fixture.pool;

    // Never released, so the timeout is reached by construction.
    let _held = pool.operation_guard();
    let _also_held = pool.operation_guard_named("a-named-one");

    let remaining = pool
        .graceful_shutdown(Duration::from_millis(200))
        .await
        .expect_err("two operations are held open, so the budget must be exhausted");

    // The count is the contract, not merely that it failed: a caller decides between
    // waiting longer and forcing the close on exactly this number.
    assert_eq!(
        remaining, 2,
        "the error names how many operations were still in flight"
    );
    assert!(
        pool.is_shutting_down(),
        "the flag is set even on the timeout path"
    );
}
