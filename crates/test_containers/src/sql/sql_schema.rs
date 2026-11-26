use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::OnceCell;

use tracing::{debug, warn};

use fundamental_db::SchemaName;
use fundamental_sql::test_support::ReclaimedSchema;
use fundamental_sql::{SqlConfig, SqlPool};

use crate::acquire_lock::AcquireLock;
use crate::sql::sql_node::SqlNode;
use crate::test_container_error::TestContainerError;

/// What marks a schema on the shared node as this harness's to remove.
///
/// Distinct from [`DATABASE_PREFIX`](super::sql_database::DATABASE_PREFIX) so the sweeps never
/// reclaim each other's fixtures.
pub(crate) const SCHEMA_PREFIX: &str = "s_";

/// How many databases the suite's schemas are spread across.
///
/// Per-database catalogs make schemas in one database conflict; eight, because the fixture
/// group admits four at a time and each may hold two.
const HOSTS: u32 = 8;

/// What a host database is called.
///
/// Outside the `t_` grammar, so the age sweep never reclaims one.
pub(super) const HOST_PREFIX: &str = "host_";

/// How many scratch databases [`ensure_every_host`] makes: four, matching `store-fixtures`'s
/// `max-threads`, so leasing never waits.
const SCRATCHES: u32 = 4;

/// What a scratch database is called.
///
/// Outside the `t_` grammar, so the age sweep never reclaims one.
const SCRATCH_PREFIX: &str = "scratch_";

static HOST: OnceCell<String> = OnceCell::const_new();

/// The second database, for the one fixture that needs to observe routing between two.
static SECOND_HOST: OnceCell<String> = OnceCell::const_new();

/// How long to wait for a host database another process created to become visible here.
///
/// A deadline rather than a count: catalog propagation is slowest when the node is busiest.
const VISIBLE_BUDGET: Duration = Duration::from_secs(30);

/// How many times to try making a host database before giving up on it.
///
/// The unit retried is "ensure it exists", never a blind `CREATE DATABASE`, which has no
/// `IF NOT EXISTS`.
const HOST_ATTEMPTS: u32 = 10;

/// Pause between attempts at making a host database, doubling to a ceiling.
const HOST_BACKOFF: Duration = Duration::from_millis(200);

/// Whether `name` is visible as a database to a connection opened right now.
///
/// A fresh connection, since the one that lost the race caches a stale catalog.
async fn host_visible_now(bootstrap: &str, name: &str) -> Result<bool, TestContainerError> {
    let fresh = SqlPool::connect(&SqlConfig::development(bootstrap))
        .await
        .map_err(|error| provision("bootstrap connection", &error))?;
    host_exists(&fresh, name).await
}

/// Whether `name` became visible as a database within [`VISIBLE_BUDGET`].
///
/// Polled, because the master knows of the database before this process's catalog does.
async fn host_visible(bootstrap: &str, name: &str) -> Result<bool, TestContainerError> {
    let deadline = Instant::now() + VISIBLE_BUDGET;
    let mut backoff = Duration::from_millis(10);
    loop {
        if host_visible_now(bootstrap, name).await? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_millis(250));
    }
}

/// Whether `name` is a database on this node, asked once.
async fn host_exists(admin: &SqlPool, name: &str) -> Result<bool, TestContainerError> {
    admin
        .fetch_all(&format!(
            "SELECT 1 FROM pg_database WHERE datname = '{name}'"
        ))
        .await
        .map(|rows| !rows.is_empty())
        .map_err(|error| provision("host lookup", &error))
}

/// This process's leases, held until the process ends.
///
/// The lock files outlive the process, which `AcquireLock::try_take` treats as free. A list,
/// since one test needs two databases.
static LEASES: OnceLock<Mutex<Vec<HostLease>>> = OnceLock::new();

struct HostLease {
    host: String,
    /// Never read; held for the process.
    _lock: AcquireLock,
}

fn leases() -> &'static Mutex<Vec<HostLease>> {
    LEASES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Takes a host database no other running process holds, and names it.
///
/// Leased rather than hashed, so concurrent fixtures never share a host; falls back to the hash
/// if every host is held.
fn lease_a_host() -> String {
    let mut held = leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for host in 0..HOSTS {
        let name = format!("{HOST_PREFIX}{host}");
        // Skip what we hold: `try_take` would break our own lock and hand the host back twice.
        if held.iter().any(|lease| lease.host == name) {
            continue;
        }
        if let Some(lock) = AcquireLock::try_take(&format!("sql-host-{host}")) {
            held.push(HostLease {
                host: name.clone(),
                _lock: lock,
            });
            return name;
        }
    }
    let fallback = format!("{HOST_PREFIX}{}", std::process::id() % HOSTS);
    warn!(
        host = %fallback,
        "test containers: every host database is leased; sharing one rather than refusing"
    );
    fallback
}

/// The URL of this process's host database, creating it the first time anybody asks.
///
/// Per process, and nextest runs each test in its own process.
pub(super) async fn host_url(node: &SqlNode) -> Result<&'static str, TestContainerError> {
    HOST.get_or_try_init(|| async { ensure_host(node, lease_a_host()).await })
        .await
        .map(String::as_str)
}

/// The scratch databases this process holds right now, by index.
///
/// Released when the fixture drops, since a scratch is wiped on acquisition. Tracked here too,
/// because `try_take` treats this pid's own lock as free.
static SCRATCH_LEASES: OnceLock<Mutex<Vec<u32>>> = OnceLock::new();

fn scratch_leases() -> &'static Mutex<Vec<u32>> {
    SCRATCH_LEASES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Takes a scratch database no other process holds, and names it with the lock that says so.
///
/// Exclusive, and it waits rather than sharing: two holders would wipe each other's store.
pub(super) fn lease_a_scratch() -> (u32, String, AcquireLock) {
    let mut held = scratch_leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for scratch in 0..SCRATCHES {
        if held.contains(&scratch) {
            continue;
        }
        if let Some(lock) = AcquireLock::try_take(&scratch_lock(scratch)) {
            held.push(scratch);
            return (scratch, format!("{SCRATCH_PREFIX}{scratch}"), lock);
        }
    }
    // Every one is held; `take` waits, treating a gone holder as absent.
    let scratch = SCRATCHES - 1;
    let lock = AcquireLock::take(&scratch_lock(scratch));
    held.push(scratch);
    (scratch, format!("{SCRATCH_PREFIX}{scratch}"), lock)
}

/// Gives back a scratch this process held, so another fixture here may take it.
pub(super) fn release_a_scratch(scratch: u32) {
    let mut held = scratch_leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.retain(|held| *held != scratch);
}

fn scratch_lock(scratch: u32) -> String {
    format!("sql-scratch-{scratch}")
}

/// Makes the scratch database `name` exist if [`ensure_every_host`] did not, and answers its URL.
pub(super) async fn scratch_url(
    node: &SqlNode,
    name: String,
) -> Result<String, TestContainerError> {
    ensure_host(node, name).await
}

/// The URL of a **second** host database this process holds, leased on first use.
///
/// For the one question one database cannot answer: whether a repository routes by domain.
async fn second_host_url(node: &SqlNode) -> Result<&'static str, TestContainerError> {
    SECOND_HOST
        .get_or_try_init(|| async { ensure_host(node, lease_a_host()).await })
        .await
        .map(String::as_str)
}

/// Makes every host database exist, before any fixture is allowed to provision.
///
/// `CREATE DATABASE` is global-impact DDL, so it is issued once, serialised, before the suite
/// runs.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(super) async fn ensure_every_host(node: &SqlNode) {
    let names = (0..HOSTS)
        .map(|host| format!("{HOST_PREFIX}{host}"))
        // The scratch databases too, for the same reason.
        .chain((0..SCRATCHES).map(|scratch| format!("{SCRATCH_PREFIX}{scratch}")));
    for name in names {
        match ensure_host(node, name.clone()).await {
            Ok(_) => debug!(host = %name, "test containers: host database ready"),
            // Never fails the caller: the leasing fixture makes the host on its own terms.
            Err(error) => warn!(
                host = %name, %error,
                "test containers: could not prepare a host database; the fixture that leases \
                 it will try again"
            ),
        }
    }
}

/// Creates the host database `name` if nobody has yet, and returns its URL.
#[expect(
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn ensure_host(node: &SqlNode, name: String) -> Result<String, TestContainerError> {
    {
        let bootstrap = node.bootstrap_url();

        // Under the database fixture's lock, which narrows the race without closing it.
        let _catalog = AcquireLock::take(super::CATALOG_LOCK);
        let admin = SqlPool::connect(&SqlConfig::development(bootstrap.as_str()))
            .await
            .map_err(|error| provision("bootstrap connection", &error))?;

        // `execute`, not `execute_ddl`: a blind retry of `CREATE DATABASE` reports
        // `already exists`, so the "ask, then create" pair is what repeats.
        let mut last: Option<String> = None;
        for attempt in 1..=HOST_ATTEMPTS {
            if host_exists(&admin, &name).await? {
                break;
            }
            match admin.execute(&format!("CREATE DATABASE {name}")).await {
                Ok(_) => break,
                Err(error) => {
                    // Asks the end state on a fresh connection rather than matching the
                    // engine's error, which is `XX000`, not `42P04`.
                    if host_visible_now(bootstrap.as_str(), &name).await? {
                        debug!(
                            host = %name, %error,
                            "another process created this host database first; the end state holds"
                        );
                        break;
                    }
                    debug!(
                        host = %name, attempt, of = HOST_ATTEMPTS, %error,
                        "could not create the host database; asking again"
                    );
                    last = Some(error.to_string());
                    tokio::time::sleep(HOST_BACKOFF * (1 << (attempt - 1).min(3))).await;
                }
            }
        }

        // Once, after the attempts: a create can be invisible until the catalog propagates.
        if !host_visible(bootstrap.as_str(), &name).await? {
            let detail = last.unwrap_or_else(|| "never attempted".to_owned());
            return Err(provision(&format!("host database {name}"), &detail));
        }

        Ok::<String, TestContainerError>(node.url(&name))
    }
}

/// The one connection every schema is created through, for the whole process.
///
/// Shared and small, since a pool per fixture would multiply connections on a shared node.
static ADMIN: OnceCell<SqlPool> = OnceCell::const_new();

/// The same, for the second host database.
///
/// Its own cell, because an admin pool belongs to its database.
static SECOND_ADMIN: OnceCell<SqlPool> = OnceCell::const_new();

async fn admin_in(
    cell: &'static OnceCell<SqlPool>,
    url: &str,
) -> Result<&'static SqlPool, TestContainerError> {
    cell.get_or_try_init(|| async {
        let config = SqlConfig::development(url)
            .min_connections(1)
            .max_connections(4);
        SqlPool::connect(&config)
            .await
            .map_err(|error| provision("admin connection", &error))
    })
    .await
}

/// One test's own **schema**, on the database every suite shares.
///
/// A schema is a local catalog write, so it needs no lock and no visibility poll. Hold it for
/// the whole test: the schema is removed on drop.
#[derive(Debug)]
pub struct SqlSchema {
    pool: SqlPool,
    url: String,
    /// The namespace, for a test building its own pool from [`url`](Self::url).
    schema: SchemaName,
    /// The name, and the promise to remove whatever was created under it.
    ///
    /// Last, so `pool`'s connections close before the `DROP SCHEMA`; [`ReclaimedSchema`]'s own
    /// `Drop` is the teardown.
    reclaimed: ReclaimedSchema,
}

impl SqlSchema {
    /// Creates a schema of this test's own on the shared node, and connects a pool whose
    /// [`default_schema`](fundamental_sql::SqlConfig::default_schema) is it.
    ///
    /// # Errors
    ///
    /// [`TestContainerError::Provision`] if the name cannot be minted, the schema cannot be
    /// created, or the pool cannot be opened against it. The node's own start-up failures
    /// arrive from [`SqlNode::shared`].
    pub async fn mint() -> Result<Self, TestContainerError> {
        let node = SqlNode::shared().await?;
        let url = host_url(node).await?.to_owned();
        Self::mint_in(url, &ADMIN).await
    }

    /// The same, in a **second** database, so a test can observe routing between two.
    ///
    /// It leases a second existing host rather than creating a database.
    ///
    /// # Errors
    ///
    /// As [`mint`](Self::mint).
    pub async fn mint_on_a_second_database() -> Result<Self, TestContainerError> {
        let node = SqlNode::shared().await?;
        let url = second_host_url(node).await?.to_owned();
        Self::mint_in(url, &SECOND_ADMIN).await
    }

    /// Creates a schema of this test's own in the database at `url`, and connects to it.
    ///
    /// `admin` is that database's own pool, so the schema lands where the `search_path` looks.
    async fn mint_in(
        url: String,
        admin: &'static OnceCell<SqlPool>,
    ) -> Result<Self, TestContainerError> {
        // Claimed before the schema exists, so a failure reclaims rather than leaks.
        let reclaimed = ReclaimedSchema::claim(url.as_str(), SCHEMA_PREFIX)
            .await
            .map_err(|error| provision("schema name", &error))?;
        let name = reclaimed.name().to_owned();

        // `execute_ddl`: local, but still contended.
        admin_in(admin, url.as_str())
            .await?
            .execute_ddl(&format!("CREATE SCHEMA IF NOT EXISTS {name}"))
            .await
            .map_err(|error| provision(&format!("schema {name}"), &error))?;

        let schema = SchemaName::new(name.clone())
            .map_err(|error| provision(&format!("schema name {name}"), &error))?;
        // Every connection bears the schema's name, the liveness signal a sweep reads.
        let pool = SqlPool::connect(
            &SqlConfig::development(url.as_str())
                .default_schema(schema.clone())
                .application_name(name.as_str()),
        )
        .await
        .map_err(|error| provision(&format!("pool on schema {name}"), &error))?;

        Ok(Self {
            pool,
            url,
            schema,
            reclaimed,
        })
    }

    /// The pool, already pointed at this test's schema.
    #[must_use]
    pub fn pool(&self) -> &SqlPool {
        &self.pool
    }

    /// The schema's name, for a test that has to name it in a statement.
    #[must_use]
    pub fn name(&self) -> &str {
        self.reclaimed.name()
    }

    /// This test's namespace, for a caller building its own [`SqlConfig`].
    ///
    /// A pool from [`url`](Self::url) alone resolves to `public`; pass this to
    /// [`SqlConfig::default_schema`].
    #[must_use]
    pub fn schema(&self) -> &SchemaName {
        &self.schema
    }

    /// The URL of the database this schema lives in.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// One shape for every provisioning failure, so the engine and fixture are never forgotten.
fn provision(fixture: &str, error: &dyn std::fmt::Display) -> TestContainerError {
    TestContainerError::Provision {
        engine: "sql",
        fixture: fixture.to_owned(),
        detail: error.to_string(),
    }
}
