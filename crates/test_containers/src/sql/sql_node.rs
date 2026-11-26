use std::time::Duration;

use fundamental_db::ConnectionPool;
use fundamental_sql::{SqlConfig, SqlPool};
use tokio::sync::OnceCell;
use tracing::{debug, info, warn};

use crate::engine_node::EngineNode;
use crate::once_per_run::once_per_run;
use crate::sql::sql_schema::ensure_every_host;
use crate::sql::sweep_every_database::sweep_every_database;
use crate::sql::sweep_stale_databases::sweep_stale_databases;
use crate::test_container_error::TestContainerError;

/// How long the host-side readiness poll may wait for the node to answer.
const READY_BUDGET: Duration = Duration::from_secs(30);

/// The database every connection bootstraps through, and which no test writes to.
const BOOTSTRAP_DATABASE: &str = "yugabyte";

static NODE: OnceCell<SqlNode> = OnceCell::const_new();

/// What the once-per-run preparation of this node is called.
///
/// Taken before [`CATALOG_LOCK`](super::CATALOG_LOCK), never after, so the two cannot invert.
const NODE_PREPARED: &str = "sql-node-prepared";

/// The SQL wire of the one node this workspace's suites run against.
///
/// Named and reusable, so it is one node across every binary and run.
#[derive(Debug)]
pub struct SqlNode {
    port: u16,
}

impl SqlNode {
    /// Brings the node up on first use and returns it.
    ///
    /// # Errors
    /// Returns [`TestContainerError::Start`] when Docker will not produce the container, and
    /// [`TestContainerError::NotReady`] when it did and the node never accepted SQL.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub async fn shared() -> Result<&'static Self, TestContainerError> {
        NODE.get_or_try_init(|| async {
            let node = Self {
                port: EngineNode::shared().await?.sql_port(),
            };
            node.wait_until_ready().await?;

            // Once per run under a machine-wide lock, so queued fixtures start after it.
            once_per_run(NODE_PREPARED, || async {
                // On the way in, because a killed run never reached its teardown.
                match sweep_stale_databases(&node.bootstrap_url()).await {
                    Ok(0) => debug!("test containers: no stale test databases to sweep"),
                    Ok(reclaimed) => info!(
                        reclaimed,
                        "test containers: reclaimed test databases left by earlier runs"
                    ),
                    Err(error) => {
                        warn!(%error, "test containers: could not sweep stale test databases");
                    }
                }
                // The host databases, made once on a cold node.
                ensure_every_host(&node).await;
                // Every database's schemas, so a leak nowhere revisited is still reclaimed.
                sweep_every_database(&node).await;
            })
            .await;
            Ok(node)
        })
        .await
    }

    /// The URL of the bootstrap database, through which another database is created or dropped.
    #[must_use]
    pub fn bootstrap_url(&self) -> String {
        self.url(BOOTSTRAP_DATABASE)
    }

    /// The URL of `database` on this node.
    #[must_use]
    pub fn url(&self, database: &str) -> String {
        format!(
            "postgresql://yugabyte@127.0.0.1:{}/{database}?sslmode=disable",
            self.port
        )
    }

    /// Polls until the node accepts SQL, or gives up after [`READY_BUDGET`].
    async fn wait_until_ready(&self) -> Result<(), TestContainerError> {
        let url = self.bootstrap_url();
        let deadline = tokio::time::Instant::now() + READY_BUDGET;
        let mut last = String::from("never attempted");
        while tokio::time::Instant::now() < deadline {
            match SqlPool::connect(&SqlConfig::development(url.as_str())).await {
                Ok(pool) => match pool.health_check().await {
                    Ok(()) => return Ok(()),
                    Err(error) => last = error.to_string(),
                },
                Err(error) => last = error.to_string(),
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Err(TestContainerError::NotReady {
            engine: "yugabytedb/yugabyte",
            endpoint: url,
            waited_secs: READY_BUDGET.as_secs(),
            detail: last,
        })
    }
}
