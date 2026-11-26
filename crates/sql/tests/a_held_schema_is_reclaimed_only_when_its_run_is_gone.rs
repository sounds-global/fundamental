//! A held schema is reclaimed as soon as its run is gone, and never while it is alive.
//!
//! The first test kills a child process, this binary run again on [`CHILD_URL`], with `SIGKILL`.

use std::env;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use fundamental_db::ephemeral::{EphemeralName, HeldName, stale_cutoff_millis};
use fundamental_sql::sqlx::Row;
use fundamental_sql::test_support::{
    ReclaimedSchema, SweepBudget, drop_stale_ephemeral_schemas, schema_exists,
};
use fundamental_sql::{SqlConfig, SqlPool};
use fundamental_test_containers::sql::{SqlDatabase, SqlSchema, host_database_url};
use tracing_test::traced_test;

const CHILD_URL: &str = "FUNDAMENTAL_HELD_SCHEMA_CHILD_URL";

const HELD: &str = "HELD ";

/// How long a test waits for the engine to notice a run is gone; about 0.3 s when idle.
const RELEASE_BUDGET: Duration = Duration::from_secs(30);

async fn pool_named(database_url: &str, name: &str) -> SqlPool {
    SqlPool::connect(
        &SqlConfig::development(database_url)
            .min_connections(1)
            .max_connections(1)
            .application_name(name),
    )
    .await
    .expect("a pool on the test database")
}

async fn live_schema(database_url: &str, prefix: &str) -> (ReclaimedSchema, SqlPool) {
    let reclaimed = ReclaimedSchema::claim(database_url, prefix)
        .await
        .expect("a held schema name");
    let pool = pool_named(database_url, reclaimed.name()).await;
    pool.execute_ddl(&format!("CREATE SCHEMA IF NOT EXISTS {}", reclaimed.name()))
        .await
        .expect("the schema is created");
    (reclaimed, pool)
}

async fn dead_schema(admin: &SqlPool, prefix: &str) -> String {
    let name = HeldName::mint(prefix, 63).expect("a held name").to_string();
    admin
        .execute_ddl(&format!("CREATE SCHEMA IF NOT EXISTS {name}"))
        .await
        .expect("the schema is created");
    name
}

async fn exists(pool: &SqlPool, name: &str) -> bool {
    schema_exists(pool, name)
        .await
        .expect("the catalog answers")
}

async fn lock_is_free(admin: &SqlPool, name: &str) -> bool {
    let (class, object) = HeldName::advisory_key_of(name).expect("a held name");
    let rows = admin
        .fetch_all(&format!(
            "SELECT pg_try_advisory_lock({class}, {object}) AS taken, \
                    pg_advisory_unlock({class}, {object}) AS released"
        ))
        .await
        .expect("the lock answers");
    rows[0].try_get("taken").expect("a boolean")
}

async fn still_signalled(admin: &SqlPool, name: &str) -> bool {
    let sessions = admin
        .fetch_all(&format!(
            "SELECT 1 FROM pg_stat_activity WHERE application_name = '{name}'"
        ))
        .await
        .expect("the activity view answers");
    !sessions.is_empty() || !lock_is_free(admin, name).await
}

fn spawn_holder(database_url: &str) -> (Child, String) {
    let mut child = Command::new(env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "a_child_holds_a_schema_until_it_is_killed",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_URL, database_url)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the child starts");
    let stdout = child.stdout.take().expect("the child's output");
    let name = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        // Anywhere in the line: libtest prints `test <name> ... ` first.
        .find_map(|line| {
            line.split_once(HELD)
                .map(|(_, name)| name.trim().to_owned())
        })
        .expect("the child reports the schema it holds");
    (child, name)
}

async fn kill_and_wait_for_release(mut child: Child, admin: &SqlPool, name: &str) {
    child.kill().expect("SIGKILL is delivered");
    child.wait().expect("the child is reaped");
    let deadline = Instant::now() + RELEASE_BUDGET;
    while still_signalled(admin, name).await {
        assert!(
            Instant::now() < deadline,
            "the engine still holds {name}'s lock or session {RELEASE_BUDGET:?} after its \
             process was killed"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The child: holds a schema and waits to be killed; a no-op unless [`CHILD_URL`] is set.
#[tokio::test]
async fn a_child_holds_a_schema_until_it_is_killed() {
    let Ok(database_url) = env::var(CHILD_URL) else {
        return;
    };
    let (reclaimed, _pool) = live_schema(&database_url, "kill_probe_").await;
    let mut stdout = std::io::stdout();
    writeln!(stdout, "{HELD}{}", reclaimed.name()).expect("the parent is listening");
    stdout.flush().expect("the parent is listening");
    tokio::time::sleep(Duration::from_secs(600)).await;
}

#[tokio::test]
#[traced_test]
async fn a_killed_runs_schema_is_reclaimed_by_the_next_mint_and_a_live_one_is_not() {
    // The host database `SqlSchema::mint` sweeps on its first mint, so nothing mints until the kill is noticed.
    let url = host_database_url()
        .await
        .expect("a host database")
        .to_owned();
    let admin = pool_named(&url, "kill-test-admin").await;

    let (dead, dead_name) = spawn_holder(&url);
    let (mut alive, alive_name) = spawn_holder(&url);
    assert!(exists(&admin, &dead_name).await, "{dead_name} was created");
    assert!(
        exists(&admin, &alive_name).await,
        "{alive_name} was created"
    );

    // Not asserted gone yet: any sweep may take it once both signals are gone.
    kill_and_wait_for_release(dead, &admin, &dead_name).await;

    let fixture = SqlSchema::mint()
        .await
        .expect("a schema of this test's own");
    assert_eq!(
        fixture.url(),
        url,
        "the mint provisions in this process's host"
    );

    assert!(
        !exists(&admin, &dead_name).await,
        "{dead_name}'s run is gone, so the mint's sweep must have reclaimed it"
    );
    assert!(
        exists(&admin, &alive_name).await,
        "{alive_name}'s run is alive in another process, so no sweep may drop it"
    );
    assert!(
        logs_contain("schema sweep: done"),
        "the sweep's outcome is logged"
    );
    for field in [
        "reclaimed_dead=",
        "reclaimed_aged=",
        "live=",
        "unknown=",
        "busy=",
        "capped=",
        "timed_out=",
    ] {
        assert!(logs_contain(field), "the sweep's outcome carries `{field}`");
    }

    alive.kill().expect("SIGKILL is delivered");
    alive.wait().expect("the child is reaped");
    admin
        .execute_ddl(&format!("DROP SCHEMA IF EXISTS {alive_name} CASCADE"))
        .await
        .expect("the live child's schema is cleared");
}

#[tokio::test]
async fn a_schema_whose_holder_session_ends_while_its_test_is_connected_survives() {
    let database = SqlDatabase::mint()
        .await
        .expect("a database of this test's own");
    let url = database.url().to_owned();
    let (reclaimed, pool) = live_schema(&url, "holder_gone_").await;
    let name = reclaimed.name().to_owned();

    let pool_pid: i32 = pool
        .fetch_all("SELECT pg_backend_pid() AS pid")
        .await
        .expect("the pool answers")[0]
        .try_get("pid")
        .expect("an int");
    let terminated = database
        .pool()
        .fetch_all(&format!(
            "SELECT pg_terminate_backend(pid) AS gone FROM pg_stat_activity \
             WHERE application_name = '{name}' AND pid <> {pool_pid}"
        ))
        .await
        .expect("the holder's session is terminated");
    assert_eq!(
        terminated.len(),
        1,
        "exactly the holder's session was terminated"
    );

    let deadline = Instant::now() + RELEASE_BUDGET;
    while !lock_is_free(database.pool(), &name).await {
        assert!(
            Instant::now() < deadline,
            "the terminated holder's lock was never released"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let outcome =
        drop_stale_ephemeral_schemas(&url, stale_cutoff_millis(), SweepBudget::ON_PROVISION)
            .await
            .expect("the sweep runs");
    assert!(
        exists(database.pool(), &name).await,
        "{name}'s lock is free but its test is still connected, so it must survive: {outcome:?}"
    );
    assert_eq!(outcome.reclaimed_dead, 0, "{outcome:?}");
    assert_eq!(outcome.live, 1, "{outcome:?}");
    drop(pool);
}

#[tokio::test]
async fn more_live_schemas_than_the_cap_never_hide_a_dead_one() {
    let database = SqlDatabase::mint()
        .await
        .expect("a database of this test's own");
    let url = database.url().to_owned();
    let cap = SweepBudget::ON_PROVISION.drops;

    let mut live = Vec::new();
    for _ in 0..=cap {
        live.push(live_schema(&url, "cap_live_").await);
    }
    let dead = dead_schema(database.pool(), "cap_dead_").await;

    let outcome =
        drop_stale_ephemeral_schemas(&url, stale_cutoff_millis(), SweepBudget::ON_PROVISION)
            .await
            .expect("the sweep runs");
    assert!(!exists(database.pool(), &dead).await, "{dead}: {outcome:?}");
    assert_eq!(outcome.reclaimed_dead, 1, "{outcome:?}");
    assert_eq!(outcome.live, cap + 1, "{outcome:?}");
    for (reclaimed, _) in &live {
        assert!(exists(database.pool(), reclaimed.name()).await);
    }
}

#[tokio::test]
async fn a_sweep_drops_at_most_its_cap_and_the_oldest_first() {
    let database = SqlDatabase::mint()
        .await
        .expect("a database of this test's own");
    let url = database.url().to_owned();
    let budget = SweepBudget {
        drops: 3,
        time: Duration::from_secs(30),
    };

    let mut dead = Vec::new();
    for _ in 0..5 {
        dead.push(dead_schema(database.pool(), "cap_oldest_").await);
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    let outcome = drop_stale_ephemeral_schemas(&url, stale_cutoff_millis(), budget)
        .await
        .expect("the sweep runs");
    assert_eq!(outcome.reclaimed_dead, 3, "{outcome:?}");
    assert!(outcome.capped, "{outcome:?}");
    for (index, name) in dead.iter().enumerate() {
        assert_eq!(
            exists(database.pool(), name).await,
            index >= 3,
            "{name} is number {index} by age: {outcome:?}"
        );
    }
}

#[tokio::test]
async fn two_sweeps_racing_one_dead_schema_drop_it_once() {
    let database = SqlDatabase::mint()
        .await
        .expect("a database of this test's own");
    let url = database.url().to_owned();
    let dead = dead_schema(database.pool(), "race_").await;

    let (first, second) = tokio::join!(
        drop_stale_ephemeral_schemas(&url, stale_cutoff_millis(), SweepBudget::ON_PROVISION),
        drop_stale_ephemeral_schemas(&url, stale_cutoff_millis(), SweepBudget::ON_PROVISION),
    );
    let first = first.expect("the first sweep runs");
    let second = second.expect("the second sweep runs");
    assert_eq!(
        first.reclaimed_dead + second.reclaimed_dead,
        1,
        "{first:?} {second:?}"
    );
    assert!(!exists(database.pool(), &dead).await);
}

#[tokio::test]
async fn an_unmarked_schema_is_left_to_the_age_rule() {
    let database = SqlDatabase::mint()
        .await
        .expect("a database of this test's own");
    let url = database.url().to_owned();
    let unmarked = EphemeralName::mint("old_pin_", 63)
        .expect("an ephemeral name")
        .to_string();
    database
        .pool()
        .execute_ddl(&format!("CREATE SCHEMA IF NOT EXISTS {unmarked}"))
        .await
        .expect("the schema is created");

    let young =
        drop_stale_ephemeral_schemas(&url, stale_cutoff_millis(), SweepBudget::ON_PROVISION)
            .await
            .expect("the sweep runs");
    assert!(
        exists(database.pool(), &unmarked).await,
        "{unmarked} is younger than the age bound and carries no lock to read: {young:?}"
    );

    let every_age =
        drop_stale_ephemeral_schemas(&url, 9_999_999_999_999, SweepBudget::ON_PROVISION)
            .await
            .expect("the sweep runs");
    assert!(!exists(database.pool(), &unmarked).await, "{every_age:?}");
    assert_eq!(every_age.reclaimed_aged, 1, "{every_age:?}");
}
