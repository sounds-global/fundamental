use crate::container_name::ENGINE_FAULT;
use crate::dedicated_container::DedicatedContainer;
use crate::engine_node::recipe_named;
use crate::test_container_error::TestContainerError;

/// A YugabyteDB node a fault run owns, configured as [`EngineNode`](crate::EngineNode) is but
/// under its own name, so restarting it disturbs no suite on the shared node.
///
/// A restart publishes new host ports: read [`sql_port`](Self::sql_port) again, or dial through a
/// [`TcpProxy`](crate::TcpProxy).
#[derive(Debug)]
pub struct DedicatedEngineNode {
    container: DedicatedContainer,
}

impl DedicatedEngineNode {
    /// Starts the node (or finds it running from an earlier run) and waits until it answers.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker will not produce the container, and
    /// [`TestContainerError::NotReady`] when the engine never answered.
    pub async fn start() -> Result<Self, TestContainerError> {
        let container = DedicatedContainer::start(recipe_named(ENGINE_FAULT)).await?;
        container.until_healthy().await?;
        Ok(Self { container })
    }

    /// The published host port for the SQL wire, as of the last start.
    #[must_use]
    pub fn sql_port(&self) -> u16 {
        self.container.ports().first().copied().unwrap_or_default()
    }

    /// The published host port for the Cassandra wire, as of the last start.
    #[must_use]
    pub fn cql_port(&self) -> u16 {
        self.container.ports().get(1).copied().unwrap_or_default()
    }

    /// The container's name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.container.name()
    }

    /// Restarts the node and waits until it answers again on its new ports.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker refuses, and
    /// [`TestContainerError::NotReady`] when the engine never answered again.
    pub async fn restart(&mut self) -> Result<(), TestContainerError> {
        self.container.restart().await?;
        self.container.until_healthy().await
    }

    /// Stops the node: every connection drops and every new one is refused.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker refuses.
    pub async fn stop(&self) -> Result<(), TestContainerError> {
        self.container.stop().await
    }

    /// Starts the stopped node again and waits until it answers on its new ports.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker refuses, and
    /// [`TestContainerError::NotReady`] when the engine never answered again.
    pub async fn start_again(&mut self) -> Result<(), TestContainerError> {
        self.container.start_again().await?;
        self.container.until_healthy().await
    }
}
