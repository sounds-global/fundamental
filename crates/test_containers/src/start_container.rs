use std::net::IpAddr;
use std::time::Duration;

use testcontainers::core::IntoContainerPort;
use testcontainers::runners::AsyncRunner;
use tracing::{info, warn};

use crate::acquire_lock::AcquireLock;
use crate::container_recipe::ContainerRecipe;
use crate::install_harness_logger::install_harness_logger;
use crate::sweep_stale_containers::sweep_stale_containers;
use crate::test_container_error::TestContainerError;

/// How many times to re-attempt a start that lost a name race.
///
/// The winner's container exists by the second lookup; three covers a create still in flight.
const CONFLICT_ATTEMPTS: u32 = 3;

/// Pause between attempts, long enough for the winner's `create` to land.
const CONFLICT_BACKOFF: Duration = Duration::from_millis(500);

/// What a started container tells the host about how to reach it.
///
/// A Cassandra driver dials the advertised address, so the harness must recognise it; see
/// [`CqlNode::config`](crate::cql::CqlNode::config).
pub(crate) struct StartedContainer {
    /// The published host ports, in the order the recipe lists them.
    pub(crate) ports: Vec<u16>,
    /// The container's address on the Docker bridge network, when it has one.
    ///
    /// `None` on the host network, or when the daemon would not say; a failure is logged.
    pub(crate) bridge_ip: Option<IpAddr>,
}

/// Brings up the container `recipe` describes and returns how to reach it.
///
/// In order: install the logger, take the cross-process lock, sweep containers from older
/// recipes, start retrying a name conflict, then read the ports and drop the handle; a reusable
/// container's `Drop` reaps nothing, so keeping it would control nothing.
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "readability baseline"
)]
pub(crate) async fn start_container(
    recipe: &ContainerRecipe,
) -> Result<StartedContainer, TestContainerError> {
    install_harness_logger();

    let _lock = AcquireLock::take(&recipe.name);
    sweep_stale_containers(&recipe.name, &recipe.fingerprint()).await;

    let mut last = String::new();
    for attempt in 1..=CONFLICT_ATTEMPTS {
        match recipe.request().start().await {
            Ok(container) => {
                let mut ports = Vec::with_capacity(recipe.ports.len());
                for exposed in &recipe.ports {
                    let mapped =
                        container
                            .get_host_port_ipv4(exposed.tcp())
                            .await
                            .map_err(|error| TestContainerError::NoHostPort {
                                engine: recipe.image,
                                name: recipe.name.clone(),
                                port: *exposed,
                                detail: error.to_string(),
                            })?;
                    ports.push(mapped);
                }
                // Asked before the handle goes; its absence costs only an address translation.
                let bridge_ip = match container.get_bridge_ip_address().await {
                    Ok(address) => Some(address),
                    Err(error) => {
                        warn!(
                            container = %recipe.name, %error,
                            "test containers: no bridge address for this container; a client \
                             that discovers node addresses will dial what the node advertises"
                        );
                        None
                    }
                };
                info!(
                    container = %recipe.name,
                    image = %recipe.image,
                    tag = %recipe.tag,
                    ?ports,
                    ?bridge_ip,
                    "test containers: engine ready"
                );
                drop(container);
                return Ok(StartedContainer { ports, bridge_ip });
            }
            Err(error) => {
                last = error.to_string();
                // Only a name race is retried; anything else fails once, honestly.
                if !is_name_conflict(&last) || attempt == CONFLICT_ATTEMPTS {
                    break;
                }
                warn!(
                    container = %recipe.name, attempt, error = %last,
                    "test containers: the container name is taken — another test binary \
                     is creating it; retrying"
                );
                tokio::time::sleep(CONFLICT_BACKOFF).await;
            }
        }
    }

    Err(TestContainerError::Start {
        engine: recipe.image,
        name: recipe.name.clone(),
        detail: last,
    })
}

/// Whether a failed start is Docker refusing a name that already exists.
///
/// Matched on the message, since `testcontainers` drops the status; two substrings keep a false
/// positive from becoming a retry loop.
fn is_name_conflict(detail: &str) -> bool {
    detail.contains("409") || detail.contains("already in use by container")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dockers_name_conflict_is_recognised() {
        assert!(is_name_conflict(
            "Docker responded with status code 409: Conflict. The container name \
             \"/fundamental-test-engine\" is already in use by container \"abc123\"."
        ));
    }

    #[test]
    fn a_rate_limit_is_not_a_name_conflict() {
        assert!(!is_name_conflict(
            "failed to pull the image 'sqldb/sql:v26.2.4', error: Docker \
             responded with status code 429: toomanyrequests: You have reached your \
             unauthenticated pull rate limit."
        ));
    }

    #[test]
    fn an_unreachable_daemon_is_not_a_name_conflict() {
        assert!(!is_name_conflict(
            "error trying to connect: No such file or directory (os error 2)"
        ));
    }
}
