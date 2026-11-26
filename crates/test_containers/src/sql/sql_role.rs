use fundamental_core::secrets::SecretString;
use fundamental_db::ephemeral::{EphemeralName, stale_cutoff};
use fundamental_sql::sqlx::Row;
use fundamental_sql::{SqlConfig, SqlPool};
use tracing::warn;

use crate::acquire_lock::AcquireLock;
use crate::sql::sql_auth_node::SqlAuthNode;
use crate::test_container_error::TestContainerError;

/// What marks a role — and the schema it owns — as this harness's to remove.
const ROLE_PREFIX: &str = "r_";

/// The longest a role or schema name may be on the SQL wire.
const MAX_NAME: usize = 63;

/// Serialises catalog writes on the auth node across every test process on this machine.
///
/// It cannot cover roles a secrets engine creates here, so [`create`] also asks whether a
/// write that reported failure applied.
const CATALOG_LOCK: &str = "sql-auth-catalog";

/// A role with its own password and its own schema, on the node that checks passwords.
///
/// Call [`release`](Self::release) at the end of the test; the next run's sweep takes what a
/// panicking test left.
#[derive(Debug)]
pub struct SqlRole {
    name: String,
    password: SecretString,
    port: u16,
}

impl SqlRole {
    /// Creates a role with a random password, owning a schema of the same name in the node's
    /// bootstrap database; a schema, because a lost `CREATE DATABASE` can leave an orphan keyspace.
    ///
    /// # Errors
    /// [`TestContainerError::Provision`] if the node refuses to create either.
    pub async fn mint() -> Result<Self, TestContainerError> {
        let node = SqlAuthNode::shared().await?;
        let name = EphemeralName::mint(ROLE_PREFIX, MAX_NAME)
            .map_err(|error| provision("a role name", error.to_string()))?
            .as_str()
            .to_owned();
        // Hex, so it needs no quoting in DDL or a URL.
        let password = uuid::Uuid::new_v4().simple().to_string();
        let admin = SqlPool::connect(&SqlConfig::development(node.superuser_url().as_str()))
            .await
            .map_err(|error| provision("a superuser connection", error.to_string()))?;
        let _catalog = AcquireLock::take(CATALOG_LOCK);
        create(
            &admin,
            &format!("CREATE ROLE {name} WITH LOGIN PASSWORD '{password}'"),
            &format!("SELECT 1 FROM pg_roles WHERE rolname = '{name}'"),
            &format!("role {name}"),
        )
        .await?;
        create(
            &admin,
            &format!("CREATE SCHEMA {name} AUTHORIZATION {name}"),
            &format!("SELECT 1 FROM pg_namespace WHERE nspname = '{name}'"),
            &format!("schema {name}"),
        )
        .await?;
        Ok(Self {
            name,
            password: SecretString::new(password),
            port: node.port(),
        })
    }

    /// The role's name, which is also its schema's.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The role's password.
    #[must_use]
    pub fn password(&self) -> &SecretString {
        &self.password
    }

    /// The role's URL on the bootstrap database, with its password in it.
    #[must_use]
    pub fn url(&self) -> SecretString {
        SecretString::new(self.url_with_password(self.password.expose()))
    }

    /// The role's URL on the bootstrap database presenting `password`.
    #[must_use]
    pub fn url_with_password(&self, password: &str) -> String {
        format!(
            "postgresql://{name}:{password}@127.0.0.1:{port}/yugabyte?sslmode=disable",
            name = self.name,
            port = self.port
        )
    }

    /// Drops the role's schema and the role.
    ///
    /// # Errors
    /// [`TestContainerError::Provision`] if the node refuses; the next run's sweep takes what
    /// was left.
    pub async fn release(self) -> Result<(), TestContainerError> {
        let node = SqlAuthNode::shared().await?;
        drop_role(&node.superuser_url(), &self.name).await
    }
}

/// Runs `ddl`, and when it reports failure asks `exists` whether it applied anyway.
///
/// A retried catalog write that had committed answers *already exists*; the name is unique, so
/// finding it proves this call created it.
async fn create(
    admin: &SqlPool,
    ddl: &str,
    exists: &str,
    fixture: &str,
) -> Result<(), TestContainerError> {
    let Err(error) = admin.execute_ddl(ddl).await else {
        return Ok(());
    };
    match admin.fetch_all(exists).await {
        Ok(rows) if !rows.is_empty() => {
            warn!(fixture, error = ?error, "test containers: a create reported failure and had applied");
            Ok(())
        }
        _ => Err(provision(fixture, format!("{error:?}"))),
    }
}

/// Drops every role, and its schema, minted before the stale cutoff.
pub(crate) async fn sweep_stale_roles(superuser_url: &str) -> Result<usize, TestContainerError> {
    let admin = SqlPool::connect(&SqlConfig::development(superuser_url))
        .await
        .map_err(|error| provision("a superuser connection", error.to_string()))?;
    let cutoff = stale_cutoff(ROLE_PREFIX);
    let stale = admin
        .fetch_all(&format!(
            "SELECT rolname FROM pg_roles WHERE rolname LIKE '{ROLE_PREFIX}%' AND rolname < '{cutoff}'"
        ))
        .await
        .map_err(|error| provision("the stale-role listing", error.to_string()))?;
    let mut swept = 0;
    for row in &stale {
        let name: String = row.get(0);
        match drop_role(superuser_url, &name).await {
            Ok(()) => swept += 1,
            Err(error) => {
                warn!(role = %name, %error, "test containers: a stale role resisted removal")
            }
        }
    }
    Ok(swept)
}

async fn drop_role(superuser_url: &str, name: &str) -> Result<(), TestContainerError> {
    let admin = SqlPool::connect(&SqlConfig::development(superuser_url))
        .await
        .map_err(|error| provision("a superuser connection", error.to_string()))?;
    let _catalog = AcquireLock::take(CATALOG_LOCK);
    for statement in [
        format!("DROP SCHEMA IF EXISTS {name} CASCADE"),
        format!("DROP ROLE IF EXISTS {name}"),
    ] {
        admin
            .execute_ddl(&statement)
            .await
            .map_err(|error| provision(&format!("releasing {name}"), format!("{error:?}")))?;
    }
    Ok(())
}

fn provision(fixture: &str, detail: String) -> TestContainerError {
    TestContainerError::Provision {
        engine: "sql",
        fixture: fixture.to_owned(),
        detail,
    }
}
