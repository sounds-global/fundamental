use std::time::Duration;

use fundamental_nats::{NatsClient, NatsConfig, ReconnectConfig};
use tokio::sync::OnceCell;
use tracing::{debug, info, warn};

use fundamental_db::ephemeral::EphemeralName;

use crate::container_recipe::ContainerRecipe;
use crate::nats::sweep_stale_streams::sweep_stale_streams;
use crate::once_per_run::{once_per_run, run_id};
use crate::start_container::start_container;
use crate::test_container_error::TestContainerError;

/// The pinned NATS image.
///
/// An immutable patch tag.
pub(super) const IMAGE: &str = "nats";
pub(super) const TAG: &str = "2.10.29";

/// The client port, inside the container.
pub(super) const CLIENT_PORT: u16 = 4222;

/// How long the host-side readiness poll may wait for the server to accept a connection.
const READY_BUDGET: Duration = Duration::from_secs(30);

static SERVER: OnceCell<NatsServer> = OnceCell::const_new();

/// The longest a JetStream stream name may be here.
///
/// A bound the minting grammar needs, not an engine limit.
const MAX_STREAM_NAME: usize = 64;

/// The NATS server one test binary runs against.
///
/// One per binary: core NATS has no subject namespace a test can take.
#[derive(Debug)]
pub struct NatsServer {
    url: String,
    /// Which server this is.
    ///
    /// So a second [`shared`](Self::shared) naming another container is refused, not handed this
    /// one.
    container: &'static str,
}

impl NatsServer {
    /// Brings this binary's server up on first use and returns it.
    ///
    /// `container` names which server, one of the [`container_name`](crate::container_name)
    /// constants.
    ///
    /// # Errors
    /// Returns [`TestContainerError::Start`] when Docker will not produce the container,
    /// and [`TestContainerError::NotReady`] when it did and the server never accepted a
    /// connection.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub async fn shared(container: &'static str) -> Result<&'static Self, TestContainerError> {
        SERVER
            .get_or_try_init(|| async {
                let started = start_container(&recipe(container)).await?;
                let server = Self {
                    url: format!("nats://127.0.0.1:{}", started.ports[0]),
                    container,
                };
                server.wait_until_ready().await?;

                // Once per run, removing only streams whose name says they are old.
                once_per_run(container, || async {
                    match sweep_stale_streams(&server.url).await {
                        Ok(0) => debug!("test containers: no stale JetStream streams to sweep"),
                        Ok(removed) => info!(
                            removed,
                            "test containers: cleared JetStream streams left by earlier runs"
                        ),
                        Err(error) => {
                            warn!(%error, "test containers: could not sweep JetStream state");
                        }
                    }
                })
                .await;
                Ok(server)
            })
            .await
            .and_then(|server| {
                if server.container == container {
                    Ok(server)
                } else {
                    Err(TestContainerError::Provision {
                        engine: "nats",
                        fixture: format!("a second NATS server named {container}"),
                        detail: format!(
                            "this process already holds {}, and the shared cell is one per \
                             process rather than one per name — returning it would hand this \
                             suite somebody else's server",
                            server.container
                        ),
                    })
                }
            })
    }

    /// A stream name of this test's own, carrying the time it was minted.
    ///
    /// The [`EphemeralName`] grammar the SQL and CQL fixtures use, so the sweep can judge age.
    /// `base` must be a plain identifier.
    ///
    /// # Errors
    /// Returns [`TestContainerError::Provision`] if `base` is not a name this grammar can
    /// mint under.
    pub fn stream_name(base: &str) -> Result<String, TestContainerError> {
        EphemeralName::mint(&format!("{base}_"), MAX_STREAM_NAME)
            .map(|name| name.as_str().to_owned())
            .map_err(|error| TestContainerError::Provision {
                engine: "nats",
                fixture: format!("a stream name for {base}"),
                detail: error.to_string(),
            })
    }

    /// The `nats://127.0.0.1:<port>` URL for this server.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// A client config for this server.
    ///
    /// Reconnection is off, so a missing broker fails instead of hanging; every subject is under
    /// this run's namespace.
    #[must_use]
    pub fn config(&self) -> NatsConfig {
        run_scoped_config(&self.url)
    }

    /// Connects a client to this server.
    ///
    /// # Errors
    /// Returns [`TestContainerError::NotReady`] if the server refuses the connection.
    pub async fn client(&self) -> Result<NatsClient, TestContainerError> {
        NatsClient::connect(self.config())
            .await
            .map_err(|error| TestContainerError::NotReady {
                engine: IMAGE,
                endpoint: self.url.clone(),
                waited_secs: 0,
                detail: error.to_string(),
            })
    }

    /// Polls until the server accepts a connection, or gives up after [`READY_BUDGET`].
    ///
    /// The `nats` image has no probe binary, and reuse applies no wait strategy anyway.
    async fn wait_until_ready(&self) -> Result<(), TestContainerError> {
        let deadline = tokio::time::Instant::now() + READY_BUDGET;
        let mut last = String::from("never attempted");
        while tokio::time::Instant::now() < deadline {
            match NatsClient::connect(self.config()).await {
                Ok(_) => return Ok(()),
                Err(error) => last = error.to_string(),
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Err(TestContainerError::NotReady {
            engine: IMAGE,
            endpoint: self.url.clone(),
            waited_secs: READY_BUDGET.as_secs(),
            detail: last,
        })
    }
}

/// The subject namespace this test run owns.
fn run_prefix() -> String {
    format!("run-{}", run_id())
}

/// A client config for the server at `url`: a first connect that fails at once, and every
/// subject under this run's namespace.
pub(super) fn run_scoped_config(url: &str) -> NatsConfig {
    let config = NatsConfig::new(url).with_reconnect(ReconnectConfig::fail_fast());
    match config.clone().with_subject_prefix(run_prefix()) {
        Ok(prefixed) => prefixed,
        // Unreachable: `run_prefix` filters to subject-safe characters.
        Err(error) => {
            warn!(%error, "test containers: run namespace refused; sharing the flat subject space");
            config
        }
    }
}

/// What the server runs.
///
/// `-js` enables JetStream; one recipe for every NATS server, since reuse matches by name.
pub(super) fn recipe(container: &'static str) -> ContainerRecipe {
    ContainerRecipe {
        name: container.to_owned(),
        image: IMAGE,
        tag: TAG,
        ports: vec![CLIENT_PORT],
        cmd: vec!["-js"],
        healthcheck: Vec::new(),
        startup_timeout: Duration::from_secs(60),
        files: Vec::new(),
        host_network: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container_name::NATS_01_ADAPTER;

    #[test]
    fn the_recipe_takes_the_name_it_is_given() {
        assert_eq!(recipe(NATS_01_ADAPTER).name, NATS_01_ADAPTER);
    }

    #[test]
    fn jetstream_is_always_on() {
        assert!(recipe(NATS_01_ADAPTER).cmd.contains(&"-js"));
    }

    #[test]
    fn there_is_no_container_healthcheck() {
        assert!(recipe(NATS_01_ADAPTER).healthcheck.is_empty());
    }

    #[test]
    fn the_image_tag_is_immutable() {
        assert!(
            TAG.matches('.').count() >= 2,
            "{TAG} is a series tag, not a patch version; it moves under us"
        );
    }
}
