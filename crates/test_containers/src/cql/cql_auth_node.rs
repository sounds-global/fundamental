use std::time::Duration;

use fundamental_core::secrets::SecretString;
use fundamental_cql::{CqlConfig, CqlPool};
use tokio::sync::OnceCell;

use crate::auth_node::AuthNode;
use crate::cql::cql_node::addressed_config;
use crate::engine_node::IMAGE;
use crate::test_container_error::TestContainerError;

/// The Cassandra wire's default superuser, which this disposable loopback node never changes.
const SUPERUSER: &str = "cassandra";
const SUPERUSER_PASSWORD: &str = "cassandra";

const READY_BUDGET: Duration = Duration::from_secs(60);

static NODE: OnceCell<CqlAuthNode> = OnceCell::const_new();

/// The Cassandra wire of the [`AuthNode`], the node that checks passwords.
#[derive(Debug)]
pub struct CqlAuthNode {
    contact_point: String,
    advertised: Option<String>,
}

impl CqlAuthNode {
    /// Brings the node up on first use and returns it once its superuser is let in.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker will not produce the container, and
    /// [`TestContainerError::NotReady`] when it did and the node never accepted the superuser.
    pub async fn shared() -> Result<&'static Self, TestContainerError> {
        NODE.get_or_try_init(|| async {
            let engine = AuthNode::shared().await?;
            let node = Self {
                contact_point: format!("127.0.0.1:{}", engine.cql_port()),
                advertised: engine
                    .bridge_ip()
                    .map(|ip| format!("{ip}:{}", engine.cql_port())),
            };
            node.wait_until_ready().await?;
            Ok(node)
        })
        .await
    }

    /// The `127.0.0.1:<port>` contact point for this node.
    #[must_use]
    pub fn contact_point(&self) -> &str {
        &self.contact_point
    }

    /// The `host=contact point` rewrite a driver outside the container network needs, in the
    /// form [`CqlConfig::address_translation`] and its settings take.
    #[must_use]
    pub fn address_translation(&self) -> Vec<String> {
        self.advertised
            .iter()
            .map(|advertised| format!("{advertised}={}", self.contact_point))
            .collect()
    }

    /// The superuser's name and password, for a secrets engine that administers roles here.
    #[must_use]
    pub fn superuser(&self) -> (&'static str, &'static str) {
        (SUPERUSER, SUPERUSER_PASSWORD)
    }

    /// A connection config for this node presenting `user` and `password`.
    #[must_use]
    pub fn config_as(&self, user: &str, password: &str) -> CqlConfig {
        let mut config = addressed_config(&self.contact_point, self.advertised.as_deref());
        config.username = Some(SecretString::new(user.to_owned()));
        config.password = Some(SecretString::new(password.to_owned()));
        config
    }

    /// A pool on this node as the superuser, for arranging state, never for the code under test.
    ///
    /// # Errors
    /// [`TestContainerError::Provision`] when the superuser is refused.
    pub async fn admin(&self) -> Result<CqlPool, TestContainerError> {
        CqlPool::connect(&self.config_as(SUPERUSER, SUPERUSER_PASSWORD))
            .await
            .map_err(|error| TestContainerError::Provision {
                engine: "cql",
                fixture: "superuser connection".to_owned(),
                detail: error.to_string(),
            })
    }

    async fn wait_until_ready(&self) -> Result<(), TestContainerError> {
        let config = self.config_as(SUPERUSER, SUPERUSER_PASSWORD);
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
            engine: IMAGE,
            endpoint: self.contact_point.clone(),
            waited_secs: READY_BUDGET.as_secs(),
            detail: last,
        })
    }
}
