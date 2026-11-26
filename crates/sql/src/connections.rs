//! Multi-instance YSQL connections, routed by a model's `DOMAIN`, with static or Vault-dynamic credentials.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use tracing::{info, warn};

use fundamental_config::settings::{CredentialSource, DatabaseSettings, SqlSettings};
use fundamental_core::Environment;
use fundamental_core::model::{BoundTo, Connection, Model, SqlDb, SqlModel};
use fundamental_core::secrets::DatabaseUrl;
use fundamental_db::{ConnectionPool, DatabaseMetrics};
use fundamental_encryption::SharedKeyManager;
use fundamental_secrets::{LeasedCredential, RenewerHandle, VaultClient, spawn_db_lease_renewer};

use crate::{FromRow, SqlConfig, SqlError, SqlPool, SqlRepository, SqlResult, ToRow};

const DEFAULT_DOMAIN: &str = "default";

/// A pool cell; a Vault-dynamic instance's connections are swapped on credential rotation.
type PoolSlot = Arc<ArcSwap<SqlPool>>;

/// A registry of YSQL pools keyed by domain; the default pool serves the `"default"` domain.
///
/// # Example
///
/// ```rust,no_run
/// # use fundamental_sql::SqlConnections;
/// # use fundamental_encryption::KeyManager;
/// # #[derive(Clone, fundamental_macros::Model)]
/// # #[model(sql, table = "my_models")]
/// # struct MyModel {
/// #     #[model(id)]
/// #     id: uuid::Uuid,
/// # }
/// # async fn example(
/// #     config: &fundamental_config::settings::FundamentalConfig,
/// # ) -> Result<(), Box<dyn std::error::Error>> {
/// let vault = fundamental_secrets::VaultClient::from_settings(&config.secrets).await?;
/// let conns = SqlConnections::from_settings(
///     &config.database,
///     config.environment,
///     vault.as_ref(),
/// )
/// .await?;
/// let km = KeyManager::from_settings(&config.encryption, vault.as_ref()).await?;
/// let conns = conns.with_key_manager(km);
/// let repo = conns.repository::<MyModel>()?;
/// # let _ = repo;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct SqlConnections {
    default: Option<PoolSlot>,
    named: HashMap<String, PoolSlot>,
    key_manager: Option<SharedKeyManager>,
    /// Here rather than on a pool, so every pool read out of the registry carries it.
    metrics: Option<Arc<dyn DatabaseMetrics>>,
    /// Lease-renewal tasks, aborted when the last clone is dropped.
    renewers: Arc<Vec<RenewerHandle>>,
}

impl std::fmt::Debug for SqlConnections {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlConnections")
            .field("default", &self.default.is_some())
            .field("named", &self.named.keys().collect::<Vec<_>>())
            .field("has_key_manager", &self.key_manager.is_some())
            .field("renewers", &self.renewers.len())
            .finish()
    }
}

impl SqlConnections {
    /// Builds the registry from configuration, connecting every declared instance; Vault-dynamic instances need `vault`.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn from_settings(
        settings: &DatabaseSettings,
        environment: Environment,
        vault: Option<&VaultClient>,
    ) -> SqlResult<Self> {
        let mut renewers = Vec::new();

        let default = match &settings.sql {
            Some(s) => match connect_slot(s, environment, vault, &mut renewers).await {
                Ok(slot) => Some(slot),
                Err(error) => return Err(abandon(renewers, error).await),
            },
            None => None,
        };

        let mut named = HashMap::new();
        if let Some(named_pools) = &settings.named {
            for (domain, s) in &named_pools.sql {
                match connect_slot(s, environment, vault, &mut renewers).await {
                    Ok(slot) => named.insert(domain.clone(), slot),
                    Err(error) => return Err(abandon(renewers, error).await),
                };
            }
        }

        Ok(Self {
            default,
            named,
            key_manager: None,
            metrics: None,
            renewers: Arc::new(renewers),
        })
    }

    /// The registry [`from_settings`](Self::from_settings) would build, every setting checked and
    /// no pool connected, so a deploy refuses a bad release before restarting.
    ///
    /// # Errors
    /// [`SqlError::Config`] naming the instance and what is refused: a setting
    /// [`SqlConfig::from_fundamental_config`] refuses, a URL that does not parse, a certificate
    /// file that cannot be read, or a Vault-dynamic instance.
    pub fn unconnected(settings: &DatabaseSettings, environment: Environment) -> SqlResult<Self> {
        let unconnected = |domain: &str, slot: &SqlSettings| {
            unconnected_slot(slot, environment).map_err(|refused| {
                let reason = match refused {
                    SqlError::Config(reason) => reason,
                    other => other.to_string(),
                };
                SqlError::Config(format!("the `{domain}` pool: {reason}"))
            })
        };
        let default = match &settings.sql {
            Some(slot) => Some(unconnected(DEFAULT_DOMAIN, slot)?),
            None => None,
        };
        let mut named = HashMap::new();
        if let Some(named_pools) = &settings.named {
            for (domain, slot) in &named_pools.sql {
                named.insert(domain.clone(), unconnected(domain, slot)?);
            }
        }
        Ok(Self {
            default,
            named,
            key_manager: None,
            metrics: None,
            renewers: Arc::new(Vec::new()),
        })
    }

    /// Closes every pool, then revokes each leased credential they ran on.
    pub async fn close(&self) {
        for (domain, pool) in self.instances() {
            ConnectionPool::close(&pool).await;
            info!(domain = %domain, "sql pool closed");
        }
        for renewer in self.renewers.iter() {
            renewer.shutdown().await;
        }
    }

    /// Creates a registry from already-built pools.
    pub fn new(default: Option<SqlPool>, named: HashMap<String, SqlPool>) -> Self {
        Self {
            default: default.map(|p| Arc::new(ArcSwap::from_pointee(p))),
            named: named
                .into_iter()
                .map(|(k, p)| (k, Arc::new(ArcSwap::from_pointee(p))))
                .collect(),
            key_manager: None,
            metrics: None,
            renewers: Arc::new(Vec::new()),
        }
    }

    /// Registers a pool for a typed [`Connection`], keyed by its `NAME`; [`Primary`](fundamental_core::model::Primary) is the `"default"` domain.
    #[must_use]
    pub fn provide<C>(mut self, pool: SqlPool) -> Self
    where
        C: Connection,
    {
        self.named
            .insert(C::NAME.to_string(), Arc::new(ArcSwap::from_pointee(pool)));
        self
    }

    /// Attaches a key manager used by repositories built from this registry.
    pub fn with_key_manager(mut self, key_manager: SharedKeyManager) -> Self {
        self.key_manager = Some(key_manager);
        self
    }

    /// Returns the attached key manager, if any.
    pub fn key_manager(&self) -> Option<&SharedKeyManager> {
        self.key_manager.as_ref()
    }

    /// Attaches the collector every pool from this registry records through.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn DatabaseMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Returns the attached metrics collector, if any.
    #[must_use]
    pub fn metrics(&self) -> Option<&Arc<dyn DatabaseMetrics>> {
        self.metrics.as_ref()
    }

    /// Returns the pool a model routes to, or an error naming the missing domain.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn pool_for<M>(&self) -> SqlResult<SqlPool>
    where
        M: SqlModel,
    {
        self.pool_for_domain(M::DOMAIN)
    }

    /// Returns the pool registered for a domain, carrying this registry's key manager and metrics.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn pool_for_domain(&self, domain: &str) -> SqlResult<SqlPool> {
        if let Some(slot) = self.named.get(domain) {
            return Ok(self.attach_registry_context(SqlPool::clone(&slot.load())));
        }
        if domain == DEFAULT_DOMAIN
            && let Some(slot) = &self.default
        {
            return Ok(self.attach_registry_context(SqlPool::clone(&slot.load())));
        }
        Err(SqlError::Config(format!(
            "no YSQL connection configured for domain '{domain}'"
        )))
    }

    /// Every live pool in this registry, by instance name; no I/O, as a metrics scrape calls it.
    #[must_use]
    pub fn instances(&self) -> Vec<(String, SqlPool)> {
        let mut instances: Vec<(String, SqlPool)> = self
            .named
            .iter()
            .map(|(name, slot)| (name.clone(), SqlPool::clone(&slot.load())))
            .collect();
        if let Some(slot) = &self.default {
            instances.push((DEFAULT_DOMAIN.to_owned(), SqlPool::clone(&slot.load())));
        }
        instances.sort_by(|left, right| left.0.cmp(&right.0));
        instances
    }

    /// Attaches this registry's key manager and metrics to a pool cloned out of it.
    fn attach_registry_context(&self, pool: SqlPool) -> SqlPool {
        let pool = match &self.key_manager {
            Some(km) => pool.with_key_manager(km.clone()),
            None => pool,
        };
        match &self.metrics {
            Some(metrics) => pool.with_metrics(Arc::clone(metrics)),
            None => pool,
        }
    }

    /// Returns a repository for a model, routed to the pool for its `DOMAIN`.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn repository<M>(&self) -> SqlResult<SqlRepository<M>>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        M::Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
    {
        Ok(self.pool_for::<M>()?.repository::<M>())
    }
}

/// Connects one instance and returns its slot, registering a lease renewer for Vault dynamic credentials.
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn connect_slot(
    settings: &SqlSettings,
    environment: Environment,
    vault: Option<&VaultClient>,
    renewers: &mut Vec<RenewerHandle>,
) -> SqlResult<PoolSlot> {
    match &settings.credentials {
        CredentialSource::Static => {
            let config = SqlConfig::from_fundamental_config(settings, environment)?;
            let pool = SqlPool::connect(&config).await?;
            Ok(Arc::new(ArcSwap::from_pointee(pool)))
        }
        CredentialSource::VaultDynamic { mount, role } => {
            let vault = vault.ok_or_else(|| {
                SqlError::Config(format!(
                    "sql instance requires Vault dynamic credentials \
                     (mount='{mount}', role='{role}') but no vault client was provided"
                ))
            })?;

            let base_url = settings
                .url_template
                .clone()
                .unwrap_or_else(|| settings.url.expose().to_string());

            let creds = vault
                .dynamic_db_creds(mount, role)
                .await
                .map_err(|e| SqlError::Config(e.to_string()))?;

            let connected = match config_with_creds(settings, environment, &base_url, &creds) {
                Ok(config) => SqlPool::connect(&config).await,
                Err(error) => Err(error),
            };
            let pool = match connected {
                Ok(pool) => pool,
                Err(error) => {
                    creds
                        .revoke(vault, "the credential no pool could connect with")
                        .await;
                    return Err(error);
                }
            };
            let slot: PoolSlot = Arc::new(ArcSwap::from_pointee(pool));

            let rebuild_slot = slot.clone();
            let rebuild_settings = settings.clone();
            let rebuild_base = base_url.clone();
            let rebuild_mount = mount.clone();
            let rebuild_role = role.clone();
            let handle = spawn_db_lease_renewer(
                vault.clone(),
                mount.clone(),
                role.clone(),
                creds,
                move |new_creds: LeasedCredential| {
                    let slot = rebuild_slot.clone();
                    let settings = rebuild_settings.clone();
                    let base = rebuild_base.clone();
                    let mount = rebuild_mount.clone();
                    let role = rebuild_role.clone();
                    async move {
                        // A refused configuration keeps the running pool, rather than swapping in one aimed at the wrong namespace.
                        let config = config_with_creds(&settings, environment, &base, &new_creds)?;
                        let fresh = SqlPool::connect(&config).await?;
                        // Into the pool every handle already holds, so handles cloned before the rotation read the new connections.
                        let replaced = slot.load().rotate_to(&fresh);
                        log_pool_rotated(&mount, &role);
                        for pool in replaced.into_pools() {
                            drain(pool, &mount, &role).await;
                        }
                        Ok::<(), SqlError>(())
                    }
                },
            );
            renewers.push(handle);

            Ok(slot)
        }
    }
}

fn unconnected_slot(settings: &SqlSettings, environment: Environment) -> SqlResult<PoolSlot> {
    if let CredentialSource::VaultDynamic { mount, role } = &settings.credentials {
        return Err(SqlError::Config(format!(
            "its credential is leased from Vault (mount='{mount}', role='{role}'), which only \
             connecting asks for"
        )));
    }
    let config = SqlConfig::from_fundamental_config(settings, environment)?;
    let pool = SqlPool::connect_lazy(&config)?;
    let tls = &config.pool.tls;
    let named_files = [
        tls.ca_cert_pem.is_none().then_some(&tls.ca_cert_path),
        tls.client_cert_pem
            .is_none()
            .then_some(&tls.client_cert_path),
        tls.client_key_pem.is_none().then_some(&tls.client_key_path),
    ];
    for path in named_files.into_iter().flatten().flatten() {
        std::fs::File::open(path).map_err(|error| {
            SqlError::Config(format!(
                "the certificate file `{path}` cannot be read: {error}"
            ))
        })?;
    }
    Ok(Arc::new(ArcSwap::from_pointee(pool)))
}

/// How long replaced connections may finish their work before they close and their credential is revoked.
const DRAIN_BUDGET: Duration = Duration::from_secs(30);

/// Closes the connections a rotation replaced, waiting at most [`DRAIN_BUDGET`] for the ones in use.
async fn drain(replaced: sqlx::PgPool, mount: &str, role: &str) {
    if tokio::time::timeout(DRAIN_BUDGET, replaced.close())
        .await
        .is_err()
    {
        warn!(
            mount = %mount,
            role = %role,
            budget_secs = DRAIN_BUDGET.as_secs(),
            "connections on the replaced credential were still in use when the drain ended; \
             their credential is revoked under them"
        );
    }
}

/// Revokes the leases of the instances already connected when a later one fails; returns the failure.
async fn abandon<E>(renewers: Vec<RenewerHandle>, failure: E) -> E {
    for renewer in &renewers {
        renewer.shutdown().await;
    }
    failure
}

fn log_pool_rotated(mount: &str, role: &str) {
    info!(
        mount = %mount,
        role = %role,
        "pool reconnected and swapped after credential rotation"
    );
}

/// Builds a [`SqlConfig`] for `settings` with `creds` spliced into the URL.
///
/// # Errors
/// Whatever [`SqlConfig::from_fundamental_config`] refuses.
fn config_with_creds(
    settings: &SqlSettings,
    environment: Environment,
    base_url: &str,
    creds: &LeasedCredential,
) -> SqlResult<SqlConfig> {
    let url = inject_credentials(base_url, creds.username.expose(), creds.password.expose());
    let mut resolved = settings.clone();
    resolved.url = DatabaseUrl::new(url);
    SqlConfig::from_fundamental_config(&resolved, environment)
}

/// Splices percent-encoded `user:password` into a database URL, replacing any existing userinfo.
fn inject_credentials(base: &str, user: &str, password: &str) -> String {
    let (scheme, rest) = base.split_once("://").unwrap_or(("postgresql", base));
    let (authority, tail) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    format!(
        "{scheme}://{}:{}@{host}{tail}",
        percent_encode(user),
        percent_encode(password)
    )
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{SqlConnections, inject_credentials, log_pool_rotated, percent_encode};
    use fundamental_config::settings::{
        CredentialSource, DatabaseSettings, SqlSettings, TlsPosture,
    };
    use fundamental_core::Environment;
    use fundamental_core::secrets::DatabaseUrl;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn logs_pool_rotation_swap() {
        log_pool_rotated("reports_db", "app_rw");
        assert!(
            logs_contain("pool reconnected and swapped after credential rotation"),
            "a successful credential hot-swap must be logged so an operator can \
             confirm the rotation took effect"
        );
        assert!(logs_contain("reports_db"), "the mount must be in the event");
        assert!(logs_contain("app_rw"), "the role must be in the event");
    }

    #[test]
    fn injects_credentials_and_encodes_userinfo() {
        let url = inject_credentials(
            "postgresql://db.host:5433/app?sslmode=require",
            "svc-user",
            "p@ss/wo:rd",
        );
        assert_eq!(
            url,
            "postgresql://svc-user:p%40ss%2Fwo%3Ard@db.host:5433/app?sslmode=require"
        );
    }

    #[test]
    fn replaces_existing_userinfo() {
        let url = inject_credentials("postgresql://old:cred@host/db", "new", "pw");
        assert_eq!(url, "postgresql://new:pw@host/db");
    }

    #[test]
    fn handles_authority_only_url() {
        let url = inject_credentials("postgresql://host:5433", "u", "p");
        assert_eq!(url, "postgresql://u:p@host:5433");
    }

    fn production_pool(ca: &std::path::Path) -> SqlSettings {
        SqlSettings {
            url: DatabaseUrl::new(
                "postgresql://app:secret@db.invalid:5433/app?sslmode=verify-full",
            ),
            tls: TlsPosture::Enabled,
            tls_ca_cert_path: Some(ca.display().to_string()),
            acquire_timeout_secs: Some(5),
            ..SqlSettings::default()
        }
    }

    fn database(pool: SqlSettings) -> DatabaseSettings {
        DatabaseSettings {
            sql: Some(pool),
            ..DatabaseSettings::default()
        }
    }

    fn cluster_ca() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let ca = dir.path().join("cluster-ca.pem");
        std::fs::write(&ca, "-----BEGIN CERTIFICATE-----\n").unwrap();
        (dir, ca)
    }

    #[tokio::test]
    async fn every_pool_is_made_as_it_would_open_and_none_connects() {
        let (_dir, ca) = cluster_ca();

        let connections =
            SqlConnections::unconnected(&database(production_pool(&ca)), Environment::Production)
                .unwrap();

        assert!(connections.pool_for_domain("default").is_ok());
    }

    #[tokio::test]
    async fn a_pool_that_has_not_said_how_long_a_caller_waits_is_refused_unconnected() {
        let (_dir, ca) = cluster_ca();
        let pool = SqlSettings {
            acquire_timeout_secs: None,
            ..production_pool(&ca)
        };

        let refused =
            SqlConnections::unconnected(&database(pool), Environment::Production).unwrap_err();

        assert!(
            refused.to_string().contains("acquire_timeout_secs"),
            "{refused}"
        );
    }

    #[tokio::test]
    async fn a_certificate_the_pool_names_and_cannot_read_is_refused_unconnected() {
        let dir = tempfile::tempdir().unwrap();
        let ca = dir.path().join("cluster-ca.pem");

        let refused =
            SqlConnections::unconnected(&database(production_pool(&ca)), Environment::Production)
                .unwrap_err();

        assert!(
            refused.to_string().contains(&*ca.display().to_string()),
            "{refused}"
        );
    }

    #[tokio::test]
    async fn a_url_that_does_not_parse_is_refused_unconnected() {
        let (_dir, ca) = cluster_ca();
        let pool = SqlSettings {
            url: DatabaseUrl::new("postgresql://app:secret@db.invalid:port/app"),
            ..production_pool(&ca)
        };

        let refused =
            SqlConnections::unconnected(&database(pool), Environment::Production).unwrap_err();

        assert!(refused.to_string().contains("URL"), "{refused}");
    }

    #[tokio::test]
    async fn a_pool_on_a_leased_credential_is_refused_unconnected() {
        let (_dir, ca) = cluster_ca();
        let pool = SqlSettings {
            credentials: CredentialSource::VaultDynamic {
                mount: "database".to_owned(),
                role: "app".to_owned(),
            },
            ..production_pool(&ca)
        };

        let refused =
            SqlConnections::unconnected(&database(pool), Environment::Production).unwrap_err();

        assert!(
            refused.to_string().contains("leased from Vault"),
            "{refused}"
        );
    }

    #[tokio::test]
    async fn a_named_pool_refused_unconnected_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let ca = dir.path().join("cluster-ca.pem");
        let mut settings = DatabaseSettings::default();
        settings
            .named
            .get_or_insert_with(Default::default)
            .sql
            .insert("reports".to_owned(), production_pool(&ca));

        let refused = SqlConnections::unconnected(&settings, Environment::Production).unwrap_err();

        assert!(
            refused.to_string().contains("the `reports` pool"),
            "{refused}"
        );
    }

    #[test]
    fn percent_encode_keeps_unreserved() {
        assert_eq!(percent_encode("aZ0-._~"), "aZ0-._~");
        assert_eq!(percent_encode("a:b@c/d"), "a%3Ab%40c%2Fd");
    }
}
