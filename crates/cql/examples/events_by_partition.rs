//! Insert and query a wide-column `events` table by partition with `fundamental_cql`.
//!
//! Requires a running YCQL node at `CQL_HOSTS` (default `127.0.0.1:9042`); without one it exits
//! cleanly.
//!
//! The keyspace must exist, with a table like:
//!
//! ```cql
//! CREATE KEYSPACE IF NOT EXISTS app
//!   WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1};
//! CREATE TABLE IF NOT EXISTS app.events (
//!     tenant_id uuid,
//!     bucket    bigint,
//!     payload   text,
//!     PRIMARY KEY (tenant_id, bucket)
//! ) WITH CLUSTERING ORDER BY (bucket DESC);
//! ```
//!
//! Run with: `cargo run --example events_by_partition -p fundamental_cql`

use fundamental_cql::{Consistency, CqlConfig, CqlPool, CqlValue};
use fundamental_db::Repository;
use fundamental_macros::Model;

/// An event row in the `events` table.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(cql, table = "events")]
struct Event {
    /// Partition key — all events for a tenant live in one partition.
    #[model(partition_key)]
    tenant_id: uuid::Uuid,

    /// Clustering key — orders events within the tenant's partition.
    #[model(clustering_key)]
    bucket: i64,

    /// Opaque event body (JSON, etc.).
    payload: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hosts: Vec<String> = std::env::var("CQL_HOSTS")
        .unwrap_or_else(|_| "127.0.0.1:9042".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    // `development(...)` disables TLS for a local node; use `CqlConfig::new` against a TLS cluster.
    let config = CqlConfig::development(hosts)
        .keyspace("app")
        .default_consistency(Consistency::One)
        .application_name("events_by_partition_example");

    // With no reachable node, report and exit cleanly.
    let pool = match CqlPool::connect(&config).await {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!("no YCQL reachable ({err}); skipping live run.");
            eprintln!("start a node and set CQL_HOSTS to execute this example.");
            return Ok(());
        }
    };

    let repo = pool.repository::<Event>();

    // Write a few events into a single tenant's partition.
    let tenant_id = uuid::Uuid::new_v4();
    for bucket in 0..5 {
        repo.insert(&Event {
            tenant_id,
            bucket,
            payload: format!("event-{bucket}"),
        })
        .await?;
    }
    println!("inserted 5 events for tenant {tenant_id}");

    // Every event for this tenant, in clustering order.
    let all = pool
        .query::<Event>()
        .partition_key(CqlValue::uuid(tenant_id))
        .execute()
        .await?;
    println!("read {} events back for the partition", all.len());
    for event in &all {
        println!("  bucket={} payload={}", event.bucket, event.payload);
    }

    // Narrower read: a single row addressed by partition key + clustering key.
    let one = pool
        .query::<Event>()
        .partition_key(CqlValue::uuid(tenant_id))
        .clustering_eq("bucket", CqlValue::bigint(2))
        .execute_one()
        .await?;
    match one {
        Some(event) => println!("clustering lookup found: {event:?}"),
        None => println!("clustering lookup found no matching event"),
    }

    Ok(())
}
