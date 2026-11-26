use std::time::Duration;

use fundamental_nats::NatsClient;

use crate::container_name::NATS_08_FAULT;
use crate::dedicated_container::DedicatedContainer;
use crate::nats::nats_server::{IMAGE, recipe, run_scoped_config};
use crate::test_container_error::TestContainerError;

/// How long a started server may take to accept a connection.
const READY_BUDGET: Duration = Duration::from_secs(30);

/// A NATS server a fault run owns, configured as [`NatsServer`](crate::nats::NatsServer) is
/// but under its own name, so stopping it disturbs no suite.
///
/// A start publishes new host ports: read [`url`](Self::url) again, or dial through a
/// [`TcpProxy`](crate::TcpProxy).
#[derive(Debug)]
pub struct DedicatedNatsServer {
    container: DedicatedContainer,
}

impl DedicatedNatsServer {
    /// Starts the server, or finds it running, and waits until it accepts a connection.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker will not produce the container, and
    /// [`TestContainerError::NotReady`] when the server never accepted a connection.
    pub async fn start() -> Result<Self, TestContainerError> {
        let server = Self {
            container: DedicatedContainer::start(recipe(NATS_08_FAULT)).await?,
        };
        server.until_ready().await?;
        Ok(server)
    }

    /// The published host port, as of the last start.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.container.ports().first().copied().unwrap_or_default()
    }

    /// The `nats://127.0.0.1:<port>` URL, as of the last start.
    #[must_use]
    pub fn url(&self) -> String {
        format!("nats://127.0.0.1:{}", self.port())
    }

    /// Stops the server: every connection drops and every new one is refused.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker refuses.
    pub async fn stop(&self) -> Result<(), TestContainerError> {
        self.container.stop().await
    }

    /// Starts the stopped server again and waits until it accepts a connection.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker refuses, and [`TestContainerError::NotReady`]
    /// when the server never accepted a connection.
    pub async fn start_again(&mut self) -> Result<(), TestContainerError> {
        self.container.start_again().await?;
        self.until_ready().await
    }

    async fn until_ready(&self) -> Result<(), TestContainerError> {
        let deadline = tokio::time::Instant::now() + READY_BUDGET;
        let mut last = String::from("never attempted");
        while tokio::time::Instant::now() < deadline {
            match NatsClient::connect(run_scoped_config(&self.url())).await {
                Ok(_) => return Ok(()),
                Err(error) => last = error.to_string(),
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Err(TestContainerError::NotReady {
            engine: IMAGE,
            endpoint: self.url(),
            waited_secs: READY_BUDGET.as_secs(),
            detail: last,
        })
    }
}
