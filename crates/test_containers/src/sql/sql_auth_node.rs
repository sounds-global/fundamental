use std::net::IpAddr;
use std::time::Duration;

use fundamental_db::ConnectionPool;
use fundamental_sql::{SqlConfig, SqlPool};
use tokio::sync::OnceCell;
use tracing::{debug, info, warn};

use crate::auth_node::AuthNode;
use crate::container_name::ENGINE_AUTH;
use crate::engine_node::IMAGE;
use crate::once_per_run::once_per_run;
use crate::sql::sql_role::sweep_stale_roles;
use crate::test_container_error::TestContainerError;

/// The superuser's password with `ysql_enable_auth`: the engine's default, on a disposable
/// loopback node.
const SUPERUSER_PASSWORD: &str = "yugabyte";

const READY_BUDGET: Duration = Duration::from_secs(60);

static NODE: OnceCell<SqlAuthNode> = OnceCell::const_new();

/// The SQL wire of the [`AuthNode`], the node that checks passwords.
///
/// Only the superuser connects by a known password; every role a test mints has its own
/// ([`SqlRole`](crate::sql::SqlRole)).
#[derive(Debug)]
pub struct SqlAuthNode {
    port: u16,
    bridge_ip: Option<IpAddr>,
}

impl SqlAuthNode {
    /// Brings the node up on first use and returns it.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker will not produce the container, and
    /// [`TestContainerError::NotReady`] when it did and the node never accepted the superuser.
    pub async fn shared() -> Result<&'static Self, TestContainerError> {
        NODE.get_or_try_init(|| async {
            let engine = AuthNode::shared().await?;
            let node = Self {
                port: engine.sql_port(),
                bridge_ip: engine.bridge_ip(),
            };
            node.wait_until_ready().await?;
            once_per_run(ENGINE_AUTH, || async {
                match sweep_stale_roles(&node.superuser_url()).await {
                    Ok(0) => debug!("test containers: no stale roles to sweep"),
                    Ok(swept) => info!(
                        swept,
                        "test containers: reclaimed roles left by earlier runs"
                    ),
                    Err(error) => warn!(%error, "test containers: could not sweep stale roles"),
                }
            })
            .await;
            Ok(node)
        })
        .await
    }

    /// The published host port.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The node's address on the Docker bridge network, or `None` for a node with none.
    #[must_use]
    pub fn bridge_ip(&self) -> Option<IpAddr> {
        self.bridge_ip
    }

    /// The superuser's password, for a secrets engine that administers roles on this node.
    #[must_use]
    pub fn superuser_password(&self) -> &'static str {
        SUPERUSER_PASSWORD
    }

    /// The superuser's URL on the bootstrap database, for arranging state, never for the code
    /// under test.
    #[must_use]
    pub fn superuser_url(&self) -> String {
        self.url_as("yugabyte", SUPERUSER_PASSWORD, "yugabyte")
    }

    /// The URL of `database` on this node as `user` presenting `password`.
    #[must_use]
    pub fn url_as(&self, user: &str, password: &str, database: &str) -> String {
        format!(
            "postgresql://{user}:{password}@127.0.0.1:{}/{database}?sslmode=disable",
            self.port
        )
    }

    async fn wait_until_ready(&self) -> Result<(), TestContainerError> {
        let url = self.superuser_url();
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
            engine: IMAGE,
            endpoint: format!("127.0.0.1:{}", self.port),
            waited_secs: READY_BUDGET.as_secs(),
            detail: last,
        })
    }
}
