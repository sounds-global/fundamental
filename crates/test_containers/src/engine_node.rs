use std::net::IpAddr;
use std::time::Duration;

use tokio::sync::OnceCell;

use crate::container_name::ENGINE;
use crate::container_recipe::ContainerRecipe;
use crate::ensure_tablet_budget::ensure_tablet_budget;
use crate::once_per_run::once_per_run;
use crate::start_container::start_container;
use crate::test_container_error::TestContainerError;

/// The pinned engine image.
///
/// An immutable build tag, not a moving tag; not a digest, which names one platform.
pub(crate) const IMAGE: &str = "yugabytedb/yugabyte";
pub(crate) const TAG: &str = "2026.1.1.0-b91";

/// The SQL wire, inside the container.
pub(crate) const SQL_PORT: u16 = 5433;

/// The Cassandra wire, inside the container.
pub(crate) const CQL_PORT: u16 = 9042;

static NODE: OnceCell<EngineNode> = OnceCell::const_new();

/// The once-per-run key the engine's tablet ceiling is sized under.
const ENGINE_TABLET_BUDGET: &str = "engine-tablet-budget";

/// One node, both wires.
///
/// Named and reusable, so it is one node across every binary and run; each test gets its own
/// database or keyspace on it.
#[derive(Debug)]
pub struct EngineNode {
    sql_port: u16,
    cql_port: u16,
    bridge_ip: Option<IpAddr>,
}

impl EngineNode {
    /// Brings the node up on first use and returns it.
    ///
    /// # Errors
    /// Returns [`TestContainerError::Start`] when Docker will not produce the
    /// container, nearly always because Docker is not running.
    pub async fn shared() -> Result<&'static Self, TestContainerError> {
        NODE.get_or_try_init(|| async {
            let started = start_container(&recipe()).await?;
            // Once per run, and only ever raising.
            once_per_run(ENGINE_TABLET_BUDGET, ensure_tablet_budget).await;
            let [sql_port, cql_port] = started.ports.as_slice() else {
                return Err(TestContainerError::NoHostPort {
                    engine: IMAGE,
                    name: ENGINE.to_owned(),
                    port: SQL_PORT,
                    detail: format!("expected two published ports, got {}", started.ports.len()),
                });
            };
            Ok(Self {
                sql_port: *sql_port,
                cql_port: *cql_port,
                bridge_ip: started.bridge_ip,
            })
        })
        .await
    }

    /// The published host port for the SQL wire.
    #[must_use]
    pub fn sql_port(&self) -> u16 {
        self.sql_port
    }

    /// The published host port for the Cassandra wire.
    #[must_use]
    pub fn cql_port(&self) -> u16 {
        self.cql_port
    }

    /// The node's address on the Docker network, which is what it advertises to clients.
    ///
    /// A Cassandra driver opens its pool at the address `system.local` advertises. `None` when
    /// the container has no bridge address or the daemon would not say.
    #[must_use]
    pub fn bridge_ip(&self) -> Option<IpAddr> {
        self.bridge_ip
    }
}

/// What the node runs.
///
/// `--tserver_flags` makes `READ COMMITTED` mean itself and enables the TTL file filter the CQL
/// schema's expiry rests on. `ysql_enable_db_catalog_version_mode` stays off: on, concurrent
/// `CREATE DATABASE`s fail on a cold node.
fn recipe() -> ContainerRecipe {
    recipe_named(ENGINE)
}

/// The engine's recipe under `name`: the same node, owned by whoever asked for that name.
pub(crate) fn recipe_named(name: &str) -> ContainerRecipe {
    ContainerRecipe {
        name: name.to_owned(),
        image: IMAGE,
        tag: TAG,
        ports: vec![SQL_PORT, CQL_PORT],
        cmd: vec![
            "bin/yugabyted",
            "start",
            "--background=false",
            "--tserver_flags=yb_enable_read_committed_isolation=true,tablet_enable_ttl_file_filter=true",
        ],
        // Dialled at the address the node advertises: `yugabyted` refuses on `127.0.0.1`.
        healthcheck: vec![
            "bash",
            "-c",
            "bin/ysqlsh -h $(hostname -i) -p 5433 -U yugabyte -d yugabyte -c 'SELECT 1'",
        ],
        // A master, a tserver and two query layers take over a minute on a cold machine.
        startup_timeout: Duration::from_secs(300),
        files: Vec::new(),
        host_network: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recipe_names_the_shared_node() {
        assert_eq!(recipe().name, ENGINE);
    }

    #[test]
    fn the_recipe_publishes_both_wires() {
        assert_eq!(recipe().ports, vec![SQL_PORT, CQL_PORT]);
    }

    #[test]
    fn the_image_tag_is_immutable() {
        assert!(
            !TAG.contains("latest"),
            "{TAG} moves; pin the build it resolves to"
        );
    }

    #[test]
    fn the_shared_engine_keeps_its_fingerprint() {
        // Shared by every checkout on a machine: change it only on purpose, with a new image.
        assert_eq!(recipe().fingerprint(), "ad8a9a3cb89243008fe8676d2e39d88e");
    }
}
