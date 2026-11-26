//! Backend-agnostic connection pool configuration.

use std::time::Duration;

use fundamental_core::secrets::DatabaseUrl;

use crate::{TlsConfig, TlsMode};

/// Common configuration for connection pools.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// The database connection URL.
    ///
    /// A [`DatabaseUrl`], so the password is masked; [`DatabaseUrl::expose`] reads the full DSN.
    pub url: DatabaseUrl,

    /// Minimum number of connections in the pool.
    pub min_connections: u32,

    /// Maximum number of connections in the pool.
    pub max_connections: u32,

    /// Maximum time to wait for a connection.
    pub acquire_timeout: Duration,

    /// Maximum lifetime of a connection.
    pub max_lifetime: Option<Duration>,

    /// Maximum idle time before a connection is closed.
    pub idle_timeout: Option<Duration>,

    /// Whether to test connections on checkout.
    pub test_on_checkout: bool,

    /// Application name (for connection metadata).
    pub application_name: Option<String>,

    /// Connect timeout for establishing new connections.
    pub connect_timeout: Duration,

    /// Statement timeout for queries.
    pub statement_timeout: Option<Duration>,

    /// TLS configuration for secure connections.
    ///
    /// Required by default; debug builds can disable it for local databases.
    pub tls: TlsConfig,
}

impl PoolConfig {
    /// Creates a new pool configuration with the given URL.
    ///
    /// TLS is required by default.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: DatabaseUrl::new(url),
            min_connections: 1,
            max_connections: 10,
            acquire_timeout: Duration::from_secs(30),
            max_lifetime: Some(Duration::from_secs(30 * 60)), // 30 minutes
            idle_timeout: Some(Duration::from_secs(10 * 60)), // 10 minutes
            test_on_checkout: false,
            application_name: None,
            connect_timeout: Duration::from_secs(10),
            statement_timeout: None,
            tls: TlsConfig::default(),
        }
    }

    /// Creates a pool configuration for development without TLS.
    ///
    /// Development environments only.
    #[cfg(debug_assertions)]
    pub fn development(url: impl Into<String>) -> Self {
        Self {
            tls: TlsConfig::disabled(),
            ..Self::new(url)
        }
    }

    /// Sets the minimum number of connections.
    pub fn min_connections(mut self, min: u32) -> Self {
        self.min_connections = min;
        self
    }

    /// Sets the maximum number of connections.
    pub fn max_connections(mut self, max: u32) -> Self {
        self.max_connections = max;
        self
    }

    /// Sets the acquire timeout.
    pub fn acquire_timeout(mut self, timeout: Duration) -> Self {
        self.acquire_timeout = timeout;
        self
    }

    /// Sets the maximum connection lifetime.
    pub fn max_lifetime(mut self, lifetime: Option<Duration>) -> Self {
        self.max_lifetime = lifetime;
        self
    }

    /// Sets the idle timeout.
    pub fn idle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Enables testing connections on checkout.
    pub fn test_on_checkout(mut self, test: bool) -> Self {
        self.test_on_checkout = test;
        self
    }

    /// Sets the application name.
    pub fn application_name(mut self, name: impl Into<String>) -> Self {
        self.application_name = Some(name.into());
        self
    }

    /// Sets the connect timeout.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the statement timeout.
    pub fn statement_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.statement_timeout = timeout;
        self
    }

    /// Sets the TLS configuration.
    pub fn tls(mut self, tls: TlsConfig) -> Self {
        self.tls = tls;
        self
    }

    /// Sets the TLS mode.
    pub fn tls_mode(mut self, mode: TlsMode) -> Self {
        self.tls.mode = mode;
        self
    }

    /// Sets the CA certificate path for TLS verification.
    pub fn tls_ca_cert(mut self, path: impl Into<String>) -> Self {
        self.tls.ca_cert_path = Some(path.into());
        self.tls.mode = TlsMode::VerifyFull;
        self
    }

    /// Returns whether TLS is enabled.
    pub fn is_tls_enabled(&self) -> bool {
        self.tls.mode.is_enabled()
    }
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self::new("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_masks_the_dsn_password() {
        let config = PoolConfig::new("postgresql://user:hunter2@localhost:5432/db");
        let rendered = format!("{config:?}");
        assert!(
            !rendered.contains("hunter2"),
            "PoolConfig Debug must not leak the DSN password: {rendered}"
        );
        assert!(
            rendered.contains("***"),
            "PoolConfig Debug must show the masked password marker: {rendered}"
        );
    }

    #[test]
    fn expose_returns_the_full_dsn_for_connecting() {
        let config = PoolConfig::new("postgresql://user:hunter2@localhost:5432/db");
        assert_eq!(
            config.url.expose(),
            "postgresql://user:hunter2@localhost:5432/db"
        );
    }
}
