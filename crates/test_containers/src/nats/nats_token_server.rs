use std::time::Duration;

use fundamental_core::secrets::SecretString;
use fundamental_nats::NatsConfig;
use tokio::sync::OnceCell;

use crate::container_name::NATS_07_TOKEN;
use crate::container_recipe::ContainerRecipe;
use crate::nats::await_nats_ready::await_nats_ready;
use crate::nats::nats_server::{CLIENT_PORT, IMAGE, TAG, run_scoped_config};
use crate::start_container::start_container;
use crate::test_container_error::TestContainerError;

const CONFIG_PATH: &str = "/nats-token.conf";

/// The one token the server admits, guarding a disposable loopback server.
const TOKEN: &str = "app-test-token";

static SERVER: OnceCell<NatsTokenServer> = OnceCell::const_new();

/// A NATS server that admits a client presenting [`token`](Self::token) and refuses every
/// other.
#[derive(Debug)]
pub struct NatsTokenServer {
    url: String,
}

impl NatsTokenServer {
    /// Brings the server up on first use and returns it.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker will not produce the container, and
    /// [`TestContainerError::NotReady`] when the token was never let in.
    pub async fn shared() -> Result<&'static Self, TestContainerError> {
        SERVER
            .get_or_try_init(|| async {
                let started = start_container(&recipe()).await?;
                let server = Self {
                    url: format!("nats://127.0.0.1:{}", started.ports[0]),
                };
                server.wait_until_ready().await?;
                Ok(server)
            })
            .await
    }

    /// The `nats://127.0.0.1:<port>` URL for this server.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The token the server admits.
    #[must_use]
    pub fn token() -> SecretString {
        SecretString::new(TOKEN.to_owned())
    }

    /// A client config for this server with no credential, a first connect that fails at once,
    /// and every subject under this run's namespace.
    #[must_use]
    pub fn anonymous_config(&self) -> NatsConfig {
        run_scoped_config(&self.url)
    }

    async fn wait_until_ready(&self) -> Result<(), TestContainerError> {
        await_nats_ready(&self.url, &self.anonymous_config().with_token(TOKEN)).await
    }
}

fn recipe() -> ContainerRecipe {
    ContainerRecipe {
        name: NATS_07_TOKEN.to_owned(),
        image: IMAGE,
        tag: TAG,
        ports: vec![CLIENT_PORT],
        cmd: vec!["-c", CONFIG_PATH],
        healthcheck: Vec::new(),
        startup_timeout: Duration::from_secs(60),
        files: vec![(
            CONFIG_PATH,
            format!("port: {CLIENT_PORT}\nauthorization: {{ token: \"{TOKEN}\" }}\n").into_bytes(),
        )],
        host_network: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_admits_the_token_it_reports() {
        let recipe = recipe();
        let written = String::from_utf8(recipe.files[0].1.clone()).expect("utf-8");
        assert!(
            written.contains(&format!("token: \"{}\"", NatsTokenServer::token().expose())),
            "{written}"
        );
    }
}
