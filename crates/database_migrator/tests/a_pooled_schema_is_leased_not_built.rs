//! A store built once per migration set, and leased per test. Requires Docker.
//!
//! The tests share a database, each minting under its own prefix; two mint a database, because
//! for them the database is the subject.

#![cfg(not(target_arch = "wasm32"))]
// `PooledSchema` is development-only, so this suite is too.
#![cfg(debug_assertions)]

use fundamental_db::ephemeral::EphemeralName;
use fundamental_db::migration::MigrationGuard;
use fundamental_db_migrator::PooledSchema;
use fundamental_sql::sqlx::Row;
use fundamental_sql::test_support::{PooledSchemaName, SweepBudget, drop_stale_ephemeral_schemas};
use fundamental_sql::{SqlConfig, SqlPool};
use fundamental_test_containers::sql::{SqlDatabase, host_database_url};
use tempfile::TempDir;

/// A prefix nothing else on the node mints under, so counting by prefix is exact.
fn probe_prefix(test: &str) -> String {
    format!("bu_{test}_")
}

/// A migrations directory holding one table and one baseline row a wipe must not take.
fn migrations_with_a_baseline(seed: &str) -> TempDir {
    let tmp = TempDir::new().expect("a temporary directory");
    std::fs::create_dir_all(tmp.path().join("sql")).expect("the migration directory");
    std::fs::write(
        tmp.path().join("sql/0001_widgets.up.sql"),
        format!(
            "CREATE TABLE IF NOT EXISTS widgets (id BIGINT PRIMARY KEY, name TEXT NOT NULL);\n\
             INSERT INTO widgets (id, name) VALUES (0, '{seed}') ON CONFLICT DO NOTHING;\n"
        ),
    )
    .expect("the migration is written");
    tmp
}

/// Leases a schema from the pool, or says which acquire failed and why.
async fn lease(url: &str, prefix: &str, migrations: &TempDir) -> PooledSchema {
    match PooledSchema::acquire(url, prefix, migrations.path(), MigrationGuard::unenforced()).await
    {
        Ok(schema) => schema,
        Err(error) => panic!("the pool could not hand out a schema: {error}"),
    }
}

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
        .expect("the catalog answered")
        .len()
}

/// How many rows the leased schema's `widgets` holds.
async fn widgets_in(pool: &SqlPool) -> i64 {
    let row = pool
        .fetch_optional("SELECT count(*) FROM widgets")
        .await
        .expect("the count ran")
        .expect("a count always returns a row");
    row.try_get(0).expect("a count is a number")
}

/// The name seeded into the baseline row the migrations insert.
async fn baseline_name(pool: &SqlPool) -> String {
    let row = pool
        .fetch_optional("SELECT name FROM widgets WHERE id = 0")
        .await
        .expect("the read ran")
        .expect("the baseline row the migration seeded");
    row.try_get(0).expect("a name is text")
}

#[tokio::test]
async fn a_second_acquire_leases_the_schema_the_first_one_built() {
    let url = host_database_url().await.expect("a host database");
    let prefix = probe_prefix("leases_the_first");
    let migrations = migrations_with_a_baseline("first");

    let first = lease(url, &prefix, &migrations).await;
    let built = first.name().to_owned();
    drop(first);

    let second = lease(url, &prefix, &migrations).await;
    assert_eq!(
        second.name(),
        built,
        "the pool built a second schema where it could have leased the first"
    );
    drop(second);

    assert_eq!(
        schemas_named(url, &prefix).await,
        1,
        "a pool that grows per acquire is the cost this fixture exists to remove"
    );
}

#[tokio::test]
async fn two_holders_are_never_handed_the_same_schema() {
    let url = host_database_url().await.expect("a host database");
    let prefix = probe_prefix("two_holders");
    let migrations = migrations_with_a_baseline("shared");

    let one = lease(url, &prefix, &migrations).await;
    let two = lease(url, &prefix, &migrations).await;

    assert_ne!(
        one.name(),
        two.name(),
        "two live tests were handed one store, which is every assertion in both of them"
    );
    assert_eq!(
        schemas_named(url, &prefix).await,
        2,
        "the pool grows to the concurrency it actually meets, and no further"
    );
}

#[tokio::test]
async fn a_leased_schema_arrives_with_the_previous_holders_rows_gone() {
    let url = host_database_url().await.expect("a host database");
    let prefix = probe_prefix("wiped_on_lease");
    let migrations = migrations_with_a_baseline("baseline");

    let first = lease(url, &prefix, &migrations).await;
    first
        .pool()
        .execute("INSERT INTO widgets (id, name) VALUES (1, 'left behind')")
        .await
        .expect("the previous holder wrote a row");
    assert_eq!(widgets_in(first.pool()).await, 2);
    drop(first);

    let second = lease(url, &prefix, &migrations).await;
    assert_eq!(
        widgets_in(second.pool()).await,
        1,
        "the next test must see the baseline and nothing the last one wrote"
    );
}

#[tokio::test]
async fn the_baseline_a_migration_seeds_survives_the_wipe() {
    let url = host_database_url().await.expect("a host database");
    let prefix = probe_prefix("baseline_survives");
    let migrations = migrations_with_a_baseline("the singleton");

    let first = lease(url, &prefix, &migrations).await;
    drop(first);

    let second = lease(url, &prefix, &migrations).await;
    assert_eq!(
        baseline_name(second.pool()).await,
        "the singleton",
        "a leased store must be what a freshly-migrated one is, not what a delete leaves"
    );
}

#[tokio::test]
async fn a_changed_migration_set_is_never_leased_as_the_old_one() {
    let url = host_database_url().await.expect("a host database");
    let prefix = probe_prefix("changed_set");

    let before = migrations_with_a_baseline("before");
    let old = lease(url, &prefix, &before).await;
    let old_name = old.name().to_owned();
    drop(old);

    let after = migrations_with_a_baseline("after");
    let new = lease(url, &prefix, &after).await;
    assert_ne!(
        new.name(),
        old_name,
        "a schema built from other statements was handed out as this one"
    );
    assert_eq!(
        baseline_name(new.pool()).await,
        "after",
        "the new pool carries the new baseline"
    );
}

/// The sweep has no registry, so a pooled schema must not carry the ephemeral grammar. The
/// cutoff here, the year 2286, makes every ephemeral name stale.
#[tokio::test]
async fn the_age_sweep_cannot_take_a_schema_the_pool_is_holding() {
    let host = SqlDatabase::mint().await.expect("a SQL test database");
    let url = host.url().to_owned();
    let prefix = probe_prefix("age_sweep");
    let migrations = migrations_with_a_baseline("survives the sweep");

    let leased = lease(&url, &prefix, &migrations).await;
    let name = leased.name().to_owned();
    assert!(
        PooledSchemaName::is_pooled(&name),
        "{name} is not a pooled name"
    );
    assert!(
        EphemeralName::stamp_of(&name).is_none(),
        "{name} carries the grammar the age sweep removes"
    );

    let outcome = drop_stale_ephemeral_schemas(&url, 9_999_999_999_999, SweepBudget::ONCE_PER_RUN)
        .await
        .expect("the sweep ran");
    assert!(
        !outcome.busy,
        "another sweep held this database: {outcome:?}"
    );

    assert_eq!(
        widgets_in(leased.pool()).await,
        1,
        "the sweep removed a schema a live holder was still leasing"
    );
    assert_eq!(schemas_named(&url, &prefix).await, 1, "{name} was swept");
}

#[tokio::test]
async fn a_row_whose_schema_was_dropped_is_rebuilt_rather_than_leased_broken() {
    let url = host_database_url().await.expect("a host database");
    let prefix = probe_prefix("rebuilt_slot");
    let migrations = migrations_with_a_baseline("rebuilt");

    let first = lease(url, &prefix, &migrations).await;
    let name = first.name().to_owned();
    drop(first);

    // Exactly what the sweep did: the namespace goes, the registry row stays.
    SqlPool::connect(&SqlConfig::development(url))
        .await
        .expect("a connection to the database")
        .execute_ddl(&format!("DROP SCHEMA IF EXISTS {name} CASCADE"))
        .await
        .expect("the schema was dropped behind the pool's back");

    let second = lease(url, &prefix, &migrations).await;
    assert_eq!(
        second.name(),
        name,
        "the repaired slot is the one the row named"
    );
    assert_eq!(
        baseline_name(second.pool()).await,
        "rebuilt",
        "a rebuilt slot must be what a freshly-migrated store is"
    );
}

/// Both create the registry; the loser's `already exists` states the postcondition, not a failure.
#[tokio::test]
async fn two_acquires_racing_on_a_fresh_database_both_get_a_schema() {
    let host = SqlDatabase::mint().await.expect("a SQL test database");
    let url = host.url().to_owned();
    let prefix = probe_prefix("racing");
    let migrations = migrations_with_a_baseline("racing");

    let (first, second) = tokio::join!(
        PooledSchema::acquire(
            &url,
            &prefix,
            migrations.path(),
            MigrationGuard::unenforced()
        ),
        PooledSchema::acquire(
            &url,
            &prefix,
            migrations.path(),
            MigrationGuard::unenforced()
        ),
    );

    let first = first.expect("the first racer leased a schema");
    let second = second.expect("the second racer leased a schema");
    assert_ne!(
        first.name(),
        second.name(),
        "two racers were handed one store"
    );
}

/// The state is planted: a namespace with no ledger and no tables, under a registry row that
/// still names it.
#[tokio::test]
async fn a_slot_whose_build_was_interrupted_is_completed_rather_than_replayed_into() {
    let url = host_database_url().await.expect("a host database");
    let prefix = probe_prefix("interrupted_slot");
    let migrations = migrations_with_a_baseline("recovered");

    let first = lease(url, &prefix, &migrations).await;
    let slot = first.name().to_owned();
    drop(first);

    // What a build killed between `CREATE SCHEMA` and the end of its migrations leaves.
    let admin = SqlPool::connect(&SqlConfig::development(url))
        .await
        .expect("a connection to the database");
    admin
        .execute_ddl(&format!("DROP SCHEMA {slot} CASCADE"))
        .await
        .expect("the half-built state is planted");
    admin
        .execute_ddl(&format!("CREATE SCHEMA {slot}"))
        .await
        .expect("the namespace comes back without its tables");

    let second = lease(url, &prefix, &migrations).await;
    assert_eq!(
        second.name(),
        slot,
        "the incomplete slot is the one under test; a different name proves nothing"
    );
    assert_eq!(
        baseline_name(second.pool()).await,
        "recovered",
        "an interrupted slot must be migrated up and re-seeded, not left to fail every lease \
         that takes it"
    );
    assert_eq!(
        widgets_in(second.pool()).await,
        1,
        "the repaired slot carries the baseline and nothing else"
    );
}
