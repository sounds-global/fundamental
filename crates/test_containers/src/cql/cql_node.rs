use std::time::Duration;

use fundamental_cql::test_support::test_node_config;
use fundamental_cql::{Consistency, CqlConfig, CqlPool};
use tokio::sync::OnceCell;
use tracing::warn;

use crate::cql::sweep_stale_keyspaces::sweep_stale_keyspaces;
use crate::engine_node::EngineNode;
use crate::test_container_error::TestContainerError;

/// How long the host-side readiness poll may wait for the node to answer CQL.
const READY_BUDGET: Duration = Duration::from_secs(60);

static NODE: OnceCell<CqlNode> = OnceCell::const_new();

static ADMIN: OnceCell<CqlPool> = OnceCell::const_new();

/// The CQL wire of the one node this workspace's suites run against.
///
/// The same [`EngineNode`] the SQL wire is served from. Tests isolate by keyspace; RF=1 with
/// [`Consistency::One`], so quorum, failover and cross-node conflict are never exercised.
#[derive(Debug)]
pub struct CqlNode {
    contact_point: String,
    /// The `host:port` this node advertises to drivers, when it differs from the contact point.
    ///
    /// A Cassandra driver dials what the node advertises; see [`config`](Self::config).
    advertised: Option<String>,
}

impl CqlNode {
    /// Brings this binary's node up on first use and returns it.
    ///
    /// # Errors
    /// Returns [`TestContainerError::Start`] when Docker will not produce the container,
    /// and [`TestContainerError::NotReady`] when it did and the node never accepted CQL.
    pub async fn shared() -> Result<&'static Self, TestContainerError> {
        NODE.get_or_try_init(|| async {
            let engine = EngineNode::shared().await?;
            let node = Self {
                contact_point: format!("127.0.0.1:{}", engine.cql_port()),
                // The published port: the driver keeps the port it was contacted on.
                advertised: engine
                    .bridge_ip()
                    .map(|ip| format!("{ip}:{}", engine.cql_port())),
            };
            node.wait_until_ready().await?;

            // On the way in, because a killed run never reached its teardown.
            if let Err(error) = sweep_stale_keyspaces(&node.config(None)).await {
                warn!(%error, "test containers: could not sweep stale test keyspaces");
            }
            Ok(node)
        })
        .await
    }

    /// The `127.0.0.1:<port>` contact point for this node.
    #[must_use]
    pub fn contact_point(&self) -> &str {
        &self.contact_point
    }

    /// The contact points as the driver's config wants them.
    #[must_use]
    pub fn contact_points(&self) -> Vec<String> {
        vec![self.contact_point.clone()]
    }

    /// A connection config for `keyspace` on this node, or for none, as creating one needs.
    #[must_use]
    pub fn config(&self, keyspace: Option<&str>) -> CqlConfig {
        let config = addressed_config(&self.contact_point, self.advertised.as_deref());
        match keyspace {
            Some(keyspace) => config.keyspace(keyspace),
            None => config,
        }
    }

    /// The pool that addresses no keyspace, opened once per process.
    ///
    /// Opened once, because a session cannot `USE` a keyspace that does not exist yet.
    ///
    /// # Errors
    /// Returns [`TestContainerError::Provision`] if the node is up and the connection is
    /// nevertheless refused.
    pub async fn admin(&self) -> Result<&'static CqlPool, TestContainerError> {
        ADMIN
            .get_or_try_init(|| async {
                CqlPool::connect(&self.config(None)).await.map_err(|error| {
                    TestContainerError::Provision {
                        engine: "cql",
                        fixture: "admin connection".to_owned(),
                        detail: error.to_string(),
                    }
                })
            })
            .await
    }

    /// Polls until the node accepts CQL, or gives up after [`READY_BUDGET`].
    async fn wait_until_ready(&self) -> Result<(), TestContainerError> {
        let config = self.config(None);
        let deadline = tokio::time::Instant::now() + READY_BUDGET;
        let mut last = String::from("never attempted");
        while tokio::time::Instant::now() < deadline {
            match CqlPool::connect(&config).await {
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
            endpoint: self.contact_point.clone(),
            waited_secs: READY_BUDGET.as_secs(),
            detail: last,
        })
    }
}

/// A connection config for a node published at `contact_point` that advertises `advertised`.
pub(super) fn addressed_config(contact_point: &str, advertised: Option<&str>) -> CqlConfig {
    let mut config = test_node_config([contact_point]).default_consistency(Consistency::One);
    // The driver reads `system.local` and dials the node's Docker-network address at the
    // contacted port, which is refused from outside that network; translate it back.
    if let Some(advertised) = advertised {
        config.address_translation = vec![format!("{advertised}={contact_point}")];
    }
    config
}
