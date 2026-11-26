//! Multi-instance YCQL connections, routed by a model's `DOMAIN`.
//!
//! # Example
//!
//! ```rust,no_run
//! use fundamental_config::FundamentalConfig;
//! use fundamental_core::Environment;
//! use fundamental_cql::CqlConnections;
//! # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
//! # #[model(cql, table = "events")]
//! # struct MyModel {
//! #     #[model(partition_key)]
//! #     id: uuid::Uuid,
//! #     payload: String,
//! # }
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = FundamentalConfig::new(Environment::Development);
//!
//! let vault = fundamental_secrets::VaultClient::from_settings(&config.secrets).await?;
//! let conns = CqlConnections::from_settings(
//!     &config.database,
//!     config.environment,
//!     vault.as_ref(),
//! )
//! .await?;
//! let repo = conns.repository::<MyModel>()?;
//! # let _ = repo;
//! # Ok(())
//! # }
//! ```

// The doctest above documents the re-exported `CqlConnections`.
#![allow(rustdoc::private_doc_tests)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use scylla::client::session::Session;
use tracing::{info, warn};

use fundamental_config::settings::{CqlSettings, CredentialSource, DatabaseSettings};
use fundamental_core::Environment;
use fundamental_core::model::{BoundTo, Connection, CqlDb, CqlModel};
use fundamental_db::DatabaseMetrics;
use fundamental_encryption::SharedKeyManager;
use fundamental_secrets::{LeasedCredential, RenewerHandle, VaultClient, spawn_db_lease_renewer};

use crate::{CqlConfig, CqlError, CqlPool, CqlRepository, CqlResult, FromRow, ToRow};

/// The default domain — models that don't set `#[model(domain = "…")]` route here.
const DEFAULT_DOMAIN: &str = "default";

/// A swappable pool cell: a Vault-dynamic instance's session is replaced on rotation.
type PoolSlot = Arc<ArcSwap<CqlPool>>;

/// A registry of YCQL pools keyed by domain.
///
/// A model's `CqlModel::DOMAIN` selects its pool; the unnamed pool serves `"default"`.
#[derive(Clone)]
pub struct CqlConnections {
    default: Option<PoolSlot>,
    named: HashMap<String, PoolSlot>,
    key_manager: Option<SharedKeyManager>,
    /// The collector every pool taken out of this registry records through.
    ///
    /// Held here, not on a pool: a Vault-dynamic instance's pool is replaced on rotation.
    metrics: Option<Arc<dyn DatabaseMetrics>>,
    /// Background lease-renewal tasks. Shared so the registry stays `Clone`; the
    /// tasks live until the last clone is dropped, then abort.
    renewers: Arc<Vec<RenewerHandle>>,
}

impl std::fmt::Debug for CqlConnections {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CqlConnections")
            .field("default", &self.default.is_some())
            .field("named", &self.named.keys().collect::<Vec<_>>())
            .field("has_key_manager", &self.key_manager.is_some())
            .field("has_metrics", &self.metrics.is_some())
            .field("renewers", &self.renewers.len())
            .finish()
    }
}

impl CqlConnections {
    /// Builds the registry from configuration, connecting every declared instance.
    ///
    /// An instance with [`CredentialSource::VaultDynamic`] requires `vault`.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn from_settings(
        settings: &DatabaseSettings,
        environment: Environment,
        vault: Option<&VaultClient>,
    ) -> CqlResult<Self> {
        let mut renewers = Vec::new();

        let default = match &settings.cql {
            Some(s) => match connect_slot(s, environment, vault, &mut renewers).await {
                Ok(slot) => Some(slot),
                Err(error) => return Err(abandon(renewers, error).await),
            },
            None => None,
        };

        let mut named = HashMap::new();
        if let Some(named_pools) = &settings.named {
            for (domain, s) in &named_pools.cql {
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

    /// Revokes each leased credential this registry's sessions run on.
    ///
    /// For a graceful shutdown; a registry that is only dropped leaves each lease valid until its
    /// TTL.
    pub async fn close(&self) {
        for renewer in self.renewers.iter() {
            renewer.shutdown().await;
        }
    }

    /// Creates a registry from already-built pools (useful in tests and for
    /// callers that construct pools directly).
    pub fn new(default: Option<CqlPool>, named: HashMap<String, CqlPool>) -> Self {
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

    /// Registers a session for a typed [`Connection`], keyed by its `NAME`.
    ///
    /// A model declared `#[model(cql, connection = Events)]` routes to `provide::<Events>(pool)`;
    /// [`Primary`](fundamental_core::model::Primary) fills the `"default"` domain.
    #[must_use]
    pub fn provide<C>(mut self, pool: CqlPool) -> Self
    where
        C: Connection,
    {
        self.named
            .insert(C::NAME.to_string(), Arc::new(ArcSwap::from_pointee(pool)));
        self
    }

    /// Attaches a key manager used by repositories built from this registry for
    /// the field-level encryption helpers.
    pub fn with_key_manager(mut self, key_manager: SharedKeyManager) -> Self {
        self.key_manager = Some(key_manager);
        self
    }

    /// Returns the attached key manager, if any.
    pub fn key_manager(&self) -> Option<&SharedKeyManager> {
        self.key_manager.as_ref()
    }

    /// Attaches the collector every pool from this registry records through.
    ///
    /// Set here rather than on a pool, which a lease rotation replaces.
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
    ///
    /// After a credential rotation, later calls observe the new pool.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn pool_for<M>(&self) -> CqlResult<CqlPool>
    where
        M: CqlModel,
    {
        self.pool_for_domain(M::DOMAIN)
    }

    /// Returns the pool registered for a given domain name, carrying this registry's key manager.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn pool_for_domain(&self, domain: &str) -> CqlResult<CqlPool> {
        if let Some(slot) = self.named.get(domain) {
            return Ok(self.attach_registry_context(CqlPool::clone(&slot.load())));
        }
        if domain == DEFAULT_DOMAIN
            && let Some(slot) = &self.default
        {
            return Ok(self.attach_registry_context(CqlPool::clone(&slot.load())));
        }
        Err(CqlError::Config(format!(
            "no YCQL connection configured for domain '{domain}'"
        )))
    }

    /// Attaches the registry's key manager and metrics collector to a pool cloned out of it.
    fn attach_registry_context(&self, pool: CqlPool) -> CqlPool {
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
    pub fn repository<M>(&self) -> CqlResult<CqlRepository<M>>
    where
        M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
    {
        // `pool_for` carries the key manager onto the pool, so `repository::<M>()` attaches it.
        Ok(self.pool_for::<M>()?.repository::<M>())
    }
}

/// Connects a single instance and returns its swappable slot, registering a lease
/// renewer in `renewers` when the instance uses Vault dynamic credentials.
async fn connect_slot(
    settings: &CqlSettings,
    environment: Environment,
    vault: Option<&VaultClient>,
    renewers: &mut Vec<RenewerHandle>,
) -> CqlResult<PoolSlot> {
    match &settings.credentials {
        CredentialSource::Static => {
            let pool = CqlPool::connect(&CqlConfig::from_settings(settings, environment)).await?;
            Ok(Arc::new(ArcSwap::from_pointee(pool)))
        }
        CredentialSource::VaultDynamic { mount, role } => {
            let vault = vault.ok_or_else(|| {
                CqlError::Config(format!(
                    "cql instance requires Vault dynamic credentials \
                     (mount='{mount}', role='{role}') but no vault client was provided"
                ))
            })?;

            let creds = vault
                .dynamic_db_creds(mount, role)
                .await
                .map_err(|e| CqlError::Config(e.to_string()))?;

            let pool =
                match CqlPool::connect(&config_with_creds(settings, environment, &creds)).await {
                    Ok(pool) => pool,
                    Err(error) => {
                        creds
                            .revoke(vault, "the credential no session could connect with")
                            .await;
                        return Err(error);
                    }
                };
            let slot: PoolSlot = Arc::new(ArcSwap::from_pointee(pool));

            let rebuild_slot = slot.clone();
            let rebuild_settings = settings.clone();
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
                    let mount = rebuild_mount.clone();
                    let role = rebuild_role.clone();
                    async move {
                        let config = config_with_creds(&settings, environment, &new_creds);
                        let fresh = CqlPool::connect(&config).await?;
                        // Into the session every handle already holds. The renewer revokes the
                        // replaced credential once this returns, so the drain comes first.
                        let replaced = slot.load_full().rotate_to(&fresh).await;
                        log_session_rotated(&mount, &role);
                        drain(replaced, &mount, &role).await;
                        Ok::<(), CqlError>(())
                    }
                },
            );
            renewers.push(handle);

            Ok(slot)
        }
    }
}

/// Builds a [`CqlConfig`] for `settings` with `creds` applied as the session
/// username/password.
fn config_with_creds(
    settings: &CqlSettings,
    environment: Environment,
    creds: &LeasedCredential,
) -> CqlConfig {
    let mut config = CqlConfig::from_settings(settings, environment);
    config.username = Some(creds.username.clone());
    config.password = Some(creds.password.clone());
    config
}

/// How long a rotation waits for the operations still running on the replaced session.
const DRAIN_BUDGET: Duration = Duration::from_secs(30);

/// How often the drain looks whether the replaced session is still in use.
const DRAIN_POLL: Duration = Duration::from_millis(100);

/// Waits at most [`DRAIN_BUDGET`] for the operations holding the replaced session to end, then
/// drops it, which closes its connections.
///
/// Bounded because the renewer revokes the old credential next.
async fn drain(replaced: Arc<Session>, mount: &str, role: &str) {
    let deadline = tokio::time::Instant::now() + DRAIN_BUDGET;
    while Arc::strong_count(&replaced) > 1 {
        if tokio::time::Instant::now() >= deadline {
            warn!(
                mount = %mount,
                role = %role,
                budget_secs = DRAIN_BUDGET.as_secs(),
                in_use = Arc::strong_count(&replaced) - 1,
                "operations on the replaced credential were still running when the drain ended; \
                 their credential is revoked under them"
            );
            break;
        }
        tokio::time::sleep(DRAIN_POLL).await;
    }
}

/// Revokes the leases of the instances already connected when a later one fails, so a boot
/// that does not complete leaves no credential behind; returns the failure.
async fn abandon<E>(renewers: Vec<RenewerHandle>, failure: E) -> E {
    for renewer in &renewers {
        renewer.shutdown().await;
    }
    failure
}

/// Emits the success event after a credential rotation swaps in a fresh session.
fn log_session_rotated(mount: &str, role: &str) {
    info!(
        mount = %mount,
        role = %role,
        "session reconnected and swapped after credential rotation"
    );
}

#[cfg(test)]
mod tests {
    use super::log_session_rotated;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn logs_session_rotation_swap() {
        log_session_rotated("events_db", "app_rw");
        assert!(
            logs_contain("session reconnected and swapped after credential rotation"),
            "a successful credential hot-swap must be logged so an operator can \
             confirm the rotation took effect"
        );
        assert!(logs_contain("events_db"), "the mount must be in the event");
        assert!(logs_contain("app_rw"), "the role must be in the event");
    }
}
