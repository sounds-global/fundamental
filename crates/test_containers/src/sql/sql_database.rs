use fundamental_sql::sqlx::Row;
use fundamental_sql::{SqlConfig, SqlPool};

use crate::acquire_lock::AcquireLock;
use crate::sql::sql_node::SqlNode;
use crate::test_container_error::TestContainerError;

/// What marks a database on the shared node as this harness's to remove.
///
/// Nothing here mints under it now; the sweep still reclaims databases other code creates with
/// [`ReclaimedDatabase`](fundamental_sql::test_support::ReclaimedDatabase).
pub(crate) const DATABASE_PREFIX: &str = "t_";

/// One test's own database, on the node every suite shares.
///
/// For a test that needs a whole database; every other test should take a schema. Hold it for
/// the whole test: dropping it gives the database back.
#[derive(Debug)]
pub struct SqlDatabase {
    pool: SqlPool,
    url: String,
    bootstrap_url: String,
    name: String,
    /// Which scratch this is, so dropping gives it back to this process.
    scratch: u32,
    /// Dropping it releases the machine-wide claim on this database.
    _lease: AcquireLock,
}

impl SqlDatabase {
    /// Leases one of the node's scratch databases and hands it over empty.
    ///
    /// Creates nothing: `CREATE DATABASE` is global-impact DDL, so the scratches are made once
    /// per run and leased exclusively here.
    ///
    /// # Errors
    /// Returns whatever [`SqlNode::shared`] returns if the node cannot be brought up, and
    /// [`TestContainerError::Provision`] if the scratch cannot be reached or emptied.
    pub async fn mint() -> Result<Self, TestContainerError> {
        let node = SqlNode::shared().await?;
        let (scratch, name, lease) = super::sql_schema::lease_a_scratch();
        let url = super::sql_schema::scratch_url(node, name.clone()).await?;

        let pool = SqlPool::connect(&SqlConfig::development(url.as_str()))
            .await
            .map_err(|error| TestContainerError::Provision {
                engine: "sql",
                fixture: format!("connection to {name}"),
                detail: error.to_string(),
            })?;
        empty(&pool, &name).await?;

        Ok(Self {
            pool,
            url,
            bootstrap_url: node.bootstrap_url(),
            name,
            scratch,
            _lease: lease,
        })
    }

    /// The pool for this database. Clone it to hand to the code under test.
    #[must_use]
    pub fn pool(&self) -> &SqlPool {
        &self.pool
    }

    /// This database's name, for diagnostics and for DDL that has to qualify it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// This database's connection URL, for code that takes a URL rather than a pool.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The URL of the node's bootstrap database, for code that creates or drops another database.
    #[must_use]
    pub fn bootstrap_url(&self) -> &str {
        &self.bootstrap_url
    }
}

/// Empties `name` so the fixture hands out a database with nothing in it.
///
/// Every schema goes and `public` comes back empty; `DROP SCHEMA` is local to the database.
async fn empty(pool: &SqlPool, name: &str) -> Result<(), TestContainerError> {
    let provision = |detail: String| TestContainerError::Provision {
        engine: "sql",
        fixture: format!("emptying {name}"),
        detail,
    };
    let schemas = pool
        .fetch_all(
            "SELECT schema_name::text FROM information_schema.schemata \
              WHERE schema_name::text NOT LIKE 'pg\\_%' \
                AND schema_name::text <> 'information_schema'",
        )
        .await
        .map_err(|error| provision(error.to_string()))?;

    for row in &schemas {
        let schema: String = row
            .try_get(0)
            .map_err(|error| provision(error.to_string()))?;
        // Quoted, because the name comes from the catalog.
        pool.execute_ddl(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
            .await
            .map_err(|error| provision(error.to_string()))?;
    }
    pool.execute_ddl("CREATE SCHEMA IF NOT EXISTS public")
        .await
        .map_err(|error| provision(error.to_string()))?;
    Ok(())
}

/// Gives the scratch back to this process, so another fixture here may lease it.
///
/// The machine-wide claim is released by `_lease`; the database is kept and emptied by its
/// next holder.
impl Drop for SqlDatabase {
    fn drop(&mut self) {
        super::sql_schema::release_a_scratch(self.scratch);
    }
}
