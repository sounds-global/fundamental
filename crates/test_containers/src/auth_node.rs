use std::net::IpAddr;
use std::time::Duration;

use tokio::sync::OnceCell;

use crate::container_name::ENGINE_AUTH;
use crate::container_recipe::ContainerRecipe;
use crate::engine_node::{CQL_PORT, IMAGE, SQL_PORT, TAG};
use crate::start_container::start_container;
use crate::test_container_error::TestContainerError;

static NODE: OnceCell<AuthNode> = OnceCell::const_new();

/// A YugabyteDB node that checks passwords on the SQL and the Cassandra wire, for the suites
/// that must prove one is checked; the shared engine trusts every connection.
#[derive(Debug)]
pub struct AuthNode {
    sql_port: u16,
    cql_port: u16,
    bridge_ip: Option<IpAddr>,
}

impl AuthNode {
    /// Brings the node up on first use and returns it.
    ///
    /// # Errors
    /// [`TestContainerError::Start`] when Docker will not produce the container, and
    /// [`TestContainerError::NoHostPort`] when it did and published fewer ports than asked.
    pub async fn shared() -> Result<&'static Self, TestContainerError> {
        NODE.get_or_try_init(|| async {
            let started = start_container(&recipe()).await?;
            let [sql_port, cql_port] = started.ports.as_slice() else {
                return Err(TestContainerError::NoHostPort {
                    engine: IMAGE,
                    name: ENGINE_AUTH.to_owned(),
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

    /// The node's address on the Docker bridge network, which the Cassandra wire advertises, or
    /// `None` for a node with none.
    #[must_use]
    pub fn bridge_ip(&self) -> Option<IpAddr> {
        self.bridge_ip
    }
}

/// The engine with authentication on both wires.
fn recipe() -> ContainerRecipe {
    ContainerRecipe {
        name: ENGINE_AUTH.to_owned(),
        image: IMAGE,
        tag: TAG,
        ports: vec![SQL_PORT, CQL_PORT],
        cmd: vec![
            "bin/yugabyted",
            "start",
            "--background=false",
            "--tserver_flags=ysql_enable_auth=true,use_cassandra_authentication=true,\
             yb_enable_read_committed_isolation=true",
        ],
        healthcheck: vec![
            "bash",
            "-c",
            "PGPASSWORD=yugabyte bin/ysqlsh -h $(hostname -i) -p 5433 -U yugabyte -d yugabyte -c 'SELECT 1'",
        ],
        startup_timeout: Duration::from_secs(300),
        files: Vec::new(),
        host_network: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recipe_checks_passwords_on_both_wires_and_its_probe_presents_one() {
        let recipe = recipe();
        let flags = recipe
            .cmd
            .iter()
            .find(|arg| arg.starts_with("--tserver_flags="))
            .expect("the node is started with flags");
        assert!(flags.contains("ysql_enable_auth=true"), "{flags}");
        assert!(
            flags.contains("use_cassandra_authentication=true"),
            "{flags}"
        );
        assert!(
            recipe
                .healthcheck
                .iter()
                .any(|arg| arg.contains("PGPASSWORD=yugabyte"))
        );
        assert_eq!(recipe.ports, vec![SQL_PORT, CQL_PORT]);
        assert_eq!(recipe.name, ENGINE_AUTH);
    }

    #[test]
    fn the_shared_node_keeps_its_fingerprint() {
        // Shared by every checkout on a machine: a changed fingerprint recreates it under them.
        assert_eq!(recipe().fingerprint(), "ab27ca4e093511ce9516e0a84612246c");
    }
}
