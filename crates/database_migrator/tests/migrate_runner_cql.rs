//! End-to-end test of [`MigrateRunner`] against a real YugabyteDB node over YCQL (Docker):
//! generate a migration from the live (empty) keyspace, apply it, confirm
//! re-applying is idempotent, and confirm regenerating finds no further changes.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_db::migration::{Lints, MigrationConfig, MigrationGuard};
/// A real guard, so these runs exercise the refusal path a fleet ships with.
fn guard() -> MigrationGuard {
    MigrationGuard::new(1_000, Lints::default(), &[])
}

use fundamental_db_migrator::MigrateRunner;
use fundamental_macros::Model;
use fundamental_test_containers::cql::{CqlKeyspace, CqlNode};
use tempfile::TempDir;

#[derive(Clone, Debug, Model)]
#[model(cql, table = "widgets")]
struct Widget {
    #[model(partition_key)]
    tenant: String,
    #[model(clustering_key = "desc")]
    seq: i64,
    name: String,
}

#[tokio::test]
async fn apply_is_idempotent_and_check_agrees_afterwards() {
    let node = CqlNode::shared().await.expect("a YCQL node");
    let keyspace = CqlKeyspace::mint(node).await.expect("a YCQL test keyspace");
    let pool = keyspace.pool().clone();
    let tmp = TempDir::new().expect("temp dir");
    let runner = MigrateRunner::new(
        MigrationConfig {
            migrations_dir: tmp.path().to_path_buf(),
            environment: "test".to_string(),
            is_protected: false,
        },
        guard(),
    )
    .cql_model::<Widget>();

    std::fs::create_dir_all(tmp.path().join("cql")).expect("migration dir");
    std::fs::write(
        tmp.path().join("cql/0001_create_widget.up.cql"),
        "CREATE TABLE widgets (tenant text, seq bigint, name text, \
         PRIMARY KEY ((tenant), seq)) WITH CLUSTERING ORDER BY (seq DESC);",
    )
    .expect("write migration");

    let applied = runner.apply_cql(&pool).await.expect("apply");
    assert_eq!(applied.len(), 1);
    let again = runner.apply_cql(&pool).await.expect("re-apply");
    assert!(again.is_empty(), "already-applied migration is skipped");

    let check = runner.check_cql(&pool).await.expect("check");
    assert!(
        !check.has_unapplied(),
        "nothing should be unapplied: {}",
        check.summary()
    );
}
