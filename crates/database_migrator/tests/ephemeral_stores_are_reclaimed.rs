//! A store that is created and then abandoned: a failed provision leaves nothing behind.
//! Requires Docker.

#![cfg(not(target_arch = "wasm32"))]

use std::time::{Duration, Instant};

use fundamental_cql::{CqlConfig, CqlPool};
use fundamental_db::ephemeral::{EphemeralName, HeldName};
use fundamental_db::migration::MigrationGuard;
use fundamental_db_migrator::{EphemeralError, EphemeralKeyspace, EphemeralSchema};
use fundamental_sql::{SqlConfig, SqlPool};
use fundamental_test_containers::cql::{CqlKeyspace, CqlNode};
use fundamental_test_containers::sql::SqlDatabase;
use tempfile::TempDir;

/// A prefix nothing else on the node mints under, so "did anything survive" is exact.
const SCHEMA_PREFIX: &str = "rcl_schema_probe_";
/// The same, for the other wire.
const CQL_PREFIX: &str = "rcl_cql_probe_";

/// A migrations directory whose only migration fails to parse, after the store is created.
fn broken_migrations(backend: &str) -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::create_dir_all(tmp.path().join(backend)).expect("migration dir");
    std::fs::write(
        tmp.path()
            .join(format!("{backend}/0001_broken.up.{backend}")),
        "CREATE TABLE ( this is not a statement;",
    )
    .expect("write migration");
    tmp
}

/// How long to wait for a store to disappear before calling it survived.
/// A `DROP DATABASE` can take over thirty seconds, so removal is polled for.
const REMOVAL_BUDGET: Duration = Duration::from_secs(180);

/// How many schemas in `database_url` carry `prefix`.
async fn schemas_named(database_url: &str, prefix: &str) -> usize {
    SqlPool::connect(&SqlConfig::development(database_url))
        .await
        .expect("a connection to the database")
        .fetch_all(&format!(
            "SELECT schema_name::text FROM information_schema.schemata \
             WHERE schema_name::text LIKE '{prefix}%'"
        ))
        .await
        .expect("read the catalog")
        .len()
}

/// Waits for `count` to reach zero, and reports what it last saw if it never does.
async fn settles_to_none(mut count: impl AsyncFnMut() -> usize) -> usize {
    let deadline = Instant::now() + REMOVAL_BUDGET;
    loop {
        let remaining = count().await;
        if remaining == 0 || Instant::now() >= deadline {
            return remaining;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// How many keyspaces on the cluster carry `prefix`.
async fn keyspaces_named(config: &CqlConfig, prefix: &str) -> usize {
    CqlPool::connect(config)
        .await
        .expect("a bootstrap connection")
        .fetch_all::<(String,)>(&fundamental_cql::cql!(
            "SELECT keyspace_name FROM system_schema.keyspaces"
        ))
        .await
        .expect("read the schema")
        .into_iter()
        .filter(|(name,)| name.starts_with(prefix))
        .count()
}

#[tokio::test]
async fn a_provision_that_fails_after_create_leaves_no_schema_behind() {
    // A database of its own keeps the schema count exact.
    let host = SqlDatabase::mint().await.expect("a SQL test database");
    let database_url = host.url().to_owned();
    let migrations = broken_migrations("sql");

    let error = EphemeralSchema::provision(
        &database_url,
        SCHEMA_PREFIX,
        migrations.path(),
        // Deliberately unenforced: the migration under test is one this file wrote to fail
        // to parse, not an application's.
        MigrationGuard::unenforced(),
    )
    .await
    .err()
    .expect("a migration that cannot parse must fail");
    assert!(
        matches!(error, EphemeralError::Migrate { .. }),
        "the failure must land after CREATE SCHEMA, which is the window under test: {error:?}"
    );

    assert_eq!(
        settles_to_none(async || schemas_named(&database_url, SCHEMA_PREFIX).await).await,
        0,
        "the schema was created before the migration ran, so a failed provision that \
         leaves one behind leaks it"
    );
}

#[tokio::test]
async fn a_migrated_schema_is_reclaimed_when_its_guard_drops() {
    let host = SqlDatabase::mint().await.expect("a SQL test database");
    let database_url = host.url().to_owned();
    let prefix = "rcs_migrated_";

    let tmp = TempDir::new().expect("temp dir");
    std::fs::create_dir_all(tmp.path().join("sql")).expect("migration dir");
    std::fs::write(
        tmp.path().join("sql/0001_create_widget.up.sql"),
        "CREATE TABLE widgets (id BIGINT PRIMARY KEY, name TEXT NOT NULL);",
    )
    .expect("write migration");

    let name = {
        let schema = EphemeralSchema::provision(
            &database_url,
            prefix,
            tmp.path(),
            MigrationGuard::unenforced(),
        )
        .await
        .expect("a migrated schema");
        // The migration landed in the fixture's own namespace, not in `public`.
        schema
            .pool()
            .fetch_one(&format!(
                "SELECT count(*) FROM information_schema.tables \
                 WHERE table_schema = '{}' AND table_name = 'widgets'",
                schema.name()
            ))
            .await
            .expect("ask the catalog where the table went");
        schema.name().to_owned()
    };

    assert!(
        HeldName::advisory_key_of(&name).is_some(),
        "{name} must be a held name, whose lock a sweep reads, or nothing could reclaim it once \
         its run is killed"
    );
    assert_eq!(
        settles_to_none(async || schemas_named(&database_url, prefix).await).await,
        0,
        "{name} had a table in it, and `DROP SCHEMA … CASCADE` removes it without \
         terminating any session — which is why no fixture needs `pg_signal_backend`"
    );
}

#[tokio::test]
async fn a_provision_that_fails_after_create_leaves_no_keyspace_behind() {
    let node = CqlNode::shared().await.expect("a CQL node");
    let config = node.config(None);
    let migrations = broken_migrations("cql");

    let error = EphemeralKeyspace::provision(config.clone(), CQL_PREFIX, migrations.path())
        .await
        .err()
        .expect("a migration that cannot parse must fail");
    assert!(
        matches!(error, EphemeralError::Migrate { .. }),
        "the failure must land after CREATE KEYSPACE: {error:?}"
    );

    assert_eq!(
        settles_to_none(async || keyspaces_named(&config, CQL_PREFIX).await).await,
        0,
        "a failed provision must not leave a keyspace on a node that outlives the run"
    );
}

#[tokio::test]
async fn a_migrated_keyspace_is_reclaimed_when_its_guard_drops() {
    let node = CqlNode::shared().await.expect("a CQL node");
    let config = node.config(None);
    let prefix = "rck_migrated_";

    let tmp = TempDir::new().expect("temp dir");
    std::fs::create_dir_all(tmp.path().join("cql")).expect("migration dir");
    std::fs::write(
        tmp.path().join("cql/0001_create_widget.up.cql"),
        "CREATE TABLE widgets (tenant text, seq bigint, name text, \
         PRIMARY KEY ((tenant), seq));",
    )
    .expect("write migration");

    let name = {
        let keyspace = EphemeralKeyspace::provision(config.clone(), prefix, tmp.path())
            .await
            .expect("a migrated keyspace");
        keyspace.name().to_owned()
    };

    assert!(
        EphemeralName::stamp_of(&name).is_some(),
        "{name} must carry the stamp a sweep reads, or nothing could ever reclaim it"
    );
    assert_eq!(
        settles_to_none(async || keyspaces_named(&config, prefix).await).await,
        0,
        "{name} had a table in it, which is the case the bare DROP KEYSPACE could not \
         reclaim on this engine"
    );
}

/// A name with no birth time can never be reclaimed by age. The SQL half is the unit test
/// `a_claim_names_a_database_before_one_exists`.
#[tokio::test]
async fn a_minted_keyspace_is_recognisable_to_the_sweep() {
    let node = CqlNode::shared().await.expect("a CQL node");
    let keyspace = CqlKeyspace::mint(node).await.expect("a CQL test keyspace");
    assert!(
        EphemeralName::stamp_of(keyspace.name()).is_some(),
        "{} carries no birth time",
        keyspace.name()
    );
}
