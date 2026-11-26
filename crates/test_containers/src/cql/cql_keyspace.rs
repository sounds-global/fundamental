use fundamental_cql::test_support::ReclaimedKeyspace;
use fundamental_cql::{CqlIdent, CqlPool, CqlSession, ddl};

use crate::cql::cql_node::CqlNode;
use crate::test_container_error::TestContainerError;

/// What marks a keyspace on a test node as this harness's to remove.
///
/// The sweep removes only keyspaces with this prefix that are also old enough.
pub(crate) const KEYSPACE_PREFIX: &str = "t_";

/// Replication for a single-node test cluster.
const TEST_REPLICATION: &str = "{'class': 'SimpleStrategy', 'replication_factor': 1}";

/// One test's own keyspace, on the node every suite shares.
///
/// Per test, since tests run concurrently. Hold it for the whole test: it is removed on drop.
#[derive(Debug)]
pub struct CqlKeyspace {
    pool: CqlPool,
    /// The name, and the promise to remove whatever was created under it.
    ///
    /// Claimed before `CREATE KEYSPACE`. It holds addresses rather than the pool, since teardown
    /// runs outside the test's runtime, which owns the pool's connections.
    reclaimed: ReclaimedKeyspace,
}

impl CqlKeyspace {
    /// Creates a uniquely-named keyspace on `node` and connects a pool scoped to it.
    ///
    /// # Errors
    /// Returns [`TestContainerError::Provision`] if the node is healthy but the keyspace
    /// could not be created or connected to.
    pub async fn mint(node: &'static CqlNode) -> Result<Self, TestContainerError> {
        let reclaimed =
            ReclaimedKeyspace::claim(node.config(None), KEYSPACE_PREFIX).map_err(|error| {
                TestContainerError::Provision {
                    engine: "cql",
                    fixture: "keyspace name".to_owned(),
                    detail: error.to_string(),
                }
            })?;
        let name = reclaimed.name().to_owned();
        let provision = |detail: String| TestContainerError::Provision {
            engine: "cql",
            fixture: format!("keyspace {name}"),
            detail,
        };

        // The bootstrap pool addresses no keyspace: the one being created does not exist yet.
        let bootstrap = node.admin().await?;
        let keyspace = CqlIdent::new(&name).map_err(|error| provision(error.to_string()))?;
        bootstrap
            .execute_ddl(&ddl!(
                "CREATE KEYSPACE {} WITH replication = {TEST_REPLICATION}",
                keyspace
            ))
            .await
            .map_err(|error| provision(error.to_string()))?;

        let pool = CqlPool::connect(&node.config(Some(&name)))
            .await
            .map_err(|error| provision(error.to_string()))?;

        Ok(Self { pool, reclaimed })
    }

    /// The pool for this keyspace. Clone it to hand to the code under test.
    #[must_use]
    pub fn pool(&self) -> &CqlPool {
        &self.pool
    }

    /// The session for this keyspace, for DDL a repository cannot express.
    #[must_use]
    pub fn session(&self) -> &CqlSession {
        self.pool.session()
    }

    /// This keyspace's name, for diagnostics and for CQL that has to qualify it.
    #[must_use]
    pub fn name(&self) -> &str {
        self.reclaimed.name()
    }
}
