//! Connection configuration for YSQL.

use super::BatchConfig;

use std::time::Duration;

use fundamental_config::settings::{SqlSettings, TlsPosture};
use fundamental_core::Environment;
use fundamental_db::{PoolConfig, SchemaName, TlsConfig, TlsMode};

use crate::SqlError;

/// Configuration for YSQL connections.
#[derive(Debug, Clone)]
pub struct SqlConfig {
    /// Base pool configuration.
    pub pool: PoolConfig,

    /// Number of retries for serialization failures (SQLSTATE `40001`).
    pub max_retries: u32,

    /// Base delay between retries (exponential backoff).
    pub retry_base_delay: Duration,

    /// Maximum delay between retries.
    pub retry_max_delay: Duration,

    /// The schema every statement on this pool qualifies its tables with, and the connection's `search_path`.
    pub default_schema: SchemaName,

    /// Warns when a connection is held longer than this; `None` disables leak detection.
    pub leak_detection_threshold: Option<Duration>,

    /// Queries slower than this are logged with a warning.
    pub slow_query_threshold: Duration,

    /// Whether to log query text for slow queries, which may contain sensitive values.
    pub log_slow_query_text: bool,

    /// Batch operation configuration.
    pub batch: BatchConfig,
}

impl SqlConfig {
    /// Creates a new configuration with the given connection URL, with TLS required.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fundamental_sql::SqlConfig;
    ///
    /// let config = SqlConfig::new("postgresql://yugabyte@localhost:5433/myapp");
    /// ```
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            pool: PoolConfig::new(url)
                .min_connections(2)
                .max_connections(10)
                .acquire_timeout(Duration::from_secs(30))
                .max_lifetime(Some(Duration::from_secs(30 * 60)))
                .idle_timeout(Some(Duration::from_secs(10 * 60))),
            max_retries: 3,
            retry_base_delay: Duration::from_millis(10),
            retry_max_delay: Duration::from_secs(1),
            default_schema: SchemaName::default(),
            leak_detection_threshold: Some(Duration::from_secs(30)),
            slow_query_threshold: Duration::from_secs(1),
            log_slow_query_text: cfg!(debug_assertions),
            batch: BatchConfig::default(),
        }
    }

    /// Creates a configuration for development without TLS.
    #[cfg(debug_assertions)]
    pub fn development(url: impl Into<String>) -> Self {
        Self {
            pool: PoolConfig::development(url)
                .min_connections(2)
                .max_connections(10)
                .acquire_timeout(Duration::from_secs(30))
                .max_lifetime(Some(Duration::from_secs(30 * 60)))
                .idle_timeout(Some(Duration::from_secs(10 * 60))),
            max_retries: 3,
            retry_base_delay: Duration::from_millis(10),
            retry_max_delay: Duration::from_secs(1),
            default_schema: SchemaName::default(),
            leak_detection_threshold: Some(Duration::from_secs(30)),
            slow_query_threshold: Duration::from_secs(1),
            log_slow_query_text: true,
            batch: BatchConfig::default(),
        }
    }

    /// Creates configuration from `DATABASE_URL`, `DATABASE_MAX_CONNECTIONS`, `DATABASE_MIN_CONNECTIONS` and `DATABASE_CA_CERT`.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn from_env() -> Result<Self, std::env::VarError> {
        let url = std::env::var("DATABASE_URL")?;

        let mut config = Self::new(url);

        if let Ok(max) = std::env::var("DATABASE_MAX_CONNECTIONS")
            && let Ok(max) = max.parse()
        {
            config.pool = config.pool.max_connections(max);
        }

        if let Ok(min) = std::env::var("DATABASE_MIN_CONNECTIONS")
            && let Ok(min) = min.parse()
        {
            config.pool = config.pool.min_connections(min);
        }

        if let Ok(ca_cert) = std::env::var("DATABASE_CA_CERT") {
            config.pool = config.pool.tls_ca_cert(ca_cert);
        }

        Ok(config)
    }

    /// Sets the minimum number of connections.
    pub fn min_connections(mut self, min: u32) -> Self {
        self.pool = self.pool.min_connections(min);
        self
    }

    /// Sets the maximum number of connections.
    pub fn max_connections(mut self, max: u32) -> Self {
        self.pool = self.pool.max_connections(max);
        self
    }

    /// Sets the connection acquire timeout.
    pub fn acquire_timeout(mut self, timeout: Duration) -> Self {
        self.pool = self.pool.acquire_timeout(timeout);
        self
    }

    /// Sets the maximum connection lifetime.
    pub fn max_lifetime(mut self, lifetime: Option<Duration>) -> Self {
        self.pool = self.pool.max_lifetime(lifetime);
        self
    }

    /// Sets the idle timeout.
    pub fn idle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.pool = self.pool.idle_timeout(timeout);
        self
    }

    /// Sets the application name.
    pub fn application_name(mut self, name: impl Into<String>) -> Self {
        self.pool = self.pool.application_name(name);
        self
    }

    /// Sets the number of retries for serialization failures.
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Sets the retry delays.
    pub fn retry_delays(mut self, base: Duration, max: Duration) -> Self {
        self.retry_base_delay = base;
        self.retry_max_delay = max;
        self
    }

    /// Sets the schema this pool's statements qualify their tables with.
    #[must_use]
    pub fn default_schema(mut self, schema: SchemaName) -> Self {
        self.default_schema = schema;
        self
    }

    /// Sets the connection leak detection threshold; `None` disables it.
    pub fn leak_detection_threshold(mut self, threshold: Option<Duration>) -> Self {
        self.leak_detection_threshold = threshold;
        self
    }

    /// Sets the slow query threshold.
    pub fn slow_query_threshold(mut self, threshold: Duration) -> Self {
        self.slow_query_threshold = threshold;
        self
    }

    /// Sets whether to log query text for slow queries, which may contain sensitive values.
    pub fn log_slow_query_text(mut self, enable: bool) -> Self {
        self.log_slow_query_text = enable;
        self
    }

    /// Sets the batch operation configuration.
    pub fn batch(mut self, batch: BatchConfig) -> Self {
        self.batch = batch;
        self
    }

    /// Sets the statement timeout.
    pub fn statement_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.pool = self.pool.statement_timeout(timeout);
        self
    }

    /// Sets the TLS configuration.
    pub fn tls(mut self, tls: TlsConfig) -> Self {
        self.pool = self.pool.tls(tls);
        self
    }

    /// Sets the TLS mode.
    pub fn tls_mode(mut self, mode: TlsMode) -> Self {
        self.pool = self.pool.tls_mode(mode);
        self
    }

    /// Sets the CA certificate path for TLS verification, enabling `VerifyFull`.
    pub fn tls_ca_cert(mut self, path: impl Into<String>) -> Self {
        self.pool = self.pool.tls_ca_cert(path);
        self
    }

    /// Returns the connection URL, including any embedded credentials.
    pub fn url(&self) -> &str {
        self.pool.url.expose()
    }

    /// Returns whether TLS is enabled.
    pub fn is_tls_enabled(&self) -> bool {
        self.pool.is_tls_enabled()
    }

    /// Creates configuration from unified [`SqlSettings`] for `environment`.
    ///
    /// # Errors
    ///
    /// [`SqlError::Config`](crate::SqlError::Config) if `settings.schema` is not a plain identifier.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use fundamental_config::{FundamentalConfig, ConfigLoader};
    /// use fundamental_sql::SqlConfig;
    ///
    /// let fundamental_config = ConfigLoader::auto()
    ///     .with_fundamental_toml("fun.toml")
    ///     .load()?;
    ///
    /// if let Some(settings) = &fundamental_config.database.sql {
    ///     let config = SqlConfig::from_fundamental_config(settings, fundamental_config.environment)?;
    ///     // Use config to create connection pool
    /// }
    /// ```
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub fn from_fundamental_config(
        settings: &SqlSettings,
        environment: Environment,
    ) -> crate::SqlResult<Self> {
        let base = if environment.allows_insecure_tls() && !settings.tls.terminates_tls() {
            #[cfg(debug_assertions)]
            {
                Self::development(settings.url.expose())
            }
            #[cfg(not(debug_assertions))]
            {
                Self::new(settings.url.expose())
            }
        } else {
            Self::new(settings.url.expose())
        };

        // Required: a default equal to the request timeout would queue a caller as long as it had to answer.
        let acquire_timeout_secs = settings.acquire_timeout_secs.ok_or_else(|| {
            SqlError::Config(
                "this pool declares no `acquire_timeout_secs`, so how long a caller may \
                 wait for a connection is undecided. Declare it in the pool's \
                 `[apps.<app>.database.sql.<name>]` section, well under the request \
                 timeout"
                    .to_string(),
            )
        })?;

        let mut config = base
            .min_connections(settings.min_connections)
            .max_connections(settings.max_connections)
            .acquire_timeout(Duration::from_secs(acquire_timeout_secs))
            .statement_timeout(settings.statement_timeout_secs.map(Duration::from_secs))
            .idle_timeout(Some(Duration::from_secs(settings.idle_timeout_secs)));

        // Mapped, not skipped: left at `Required`, a release pool would demand a handshake the sidecar never offers.
        if settings.tls == TlsPosture::TerminatedUpstream {
            config.pool.tls = TlsConfig::terminated_upstream();
        }

        if settings.tls.terminates_tls() {
            // Inline PEM wins over a path; a configured anchor raises the mode to `VerifyFull`.
            if let Some(ca_cert_pem) = settings.tls_ca_cert_pem.as_deref() {
                config.pool.tls = config
                    .pool
                    .tls
                    .ca_cert_pem(ca_cert_pem)
                    .mode(TlsMode::VerifyFull);
            } else if let Some(ca_cert_path) = settings.tls_ca_cert_path.as_deref() {
                config = config.tls_ca_cert(ca_cert_path);
            } else {
                config = config.tls_mode(TlsMode::Required);
            }

            // Both halves or neither: half a pair fails at connect time with a misleading network error.
            match (
                settings.tls_client_cert_pem.as_deref(),
                settings.tls_client_key_pem.as_deref(),
            ) {
                (Some(cert), Some(key)) => {
                    config.pool.tls = config.pool.tls.client_cert_pem(cert, key);
                }
                _ => {
                    if let (Some(cert), Some(key)) = (
                        settings.tls_client_cert_path.as_deref(),
                        settings.tls_client_key_path.as_deref(),
                    ) {
                        config.pool.tls = config.pool.tls.client_cert(cert, key);
                    }
                }
            }
        }

        // Last, so a builder method that rebuilds the struct cannot lose it.
        if let Some(schema) = settings.schema.as_deref() {
            config.default_schema = SchemaName::new(schema).map_err(|error| {
                SqlError::Config(format!(
                    "database schema '{schema}' is not a plain identifier and cannot be \
                     interpolated into a statement: {error}"
                ))
            })?;
        }

        Ok(config)
    }
}

impl Default for SqlConfig {
    fn default() -> Self {
        Self::new("postgresql://yugabyte@localhost:5433/yugabyte")
    }
}

impl SqlConfig {
    /// Creates a `SqlMetricsConfig` with this config's slow query settings.
    pub fn to_metrics_config(&self) -> crate::metrics::SqlMetricsConfig {
        crate::metrics::SqlMetricsConfig::new()
            .slow_query_threshold(self.slow_query_threshold)
            .log_slow_query_text(self.log_slow_query_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_config::settings::{SqlSettings, TlsPosture};
    use fundamental_core::Environment;
    use fundamental_core::secrets::DatabaseUrl;

    #[test]
    fn test_config_builder() {
        let config = SqlConfig::new("postgresql://localhost:5433/test")
            .min_connections(5)
            .max_connections(20)
            .max_retries(5)
            .application_name("test-app");

        assert_eq!(config.pool.min_connections, 5);
        assert_eq!(config.pool.max_connections, 20);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.pool.application_name, Some("test-app".into()));
    }

    #[test]
    fn test_tls_default_enabled() {
        let config = SqlConfig::new("postgresql://localhost:5433/test");
        assert!(config.is_tls_enabled());
        assert_eq!(config.pool.tls.mode, TlsMode::Required);
    }

    #[test]
    fn test_tls_ca_cert() {
        let config =
            SqlConfig::new("postgresql://localhost:5433/test").tls_ca_cert("/path/to/ca.pem");

        assert!(config.is_tls_enabled());
        assert_eq!(config.pool.tls.mode, TlsMode::VerifyFull);
        assert_eq!(
            config.pool.tls.ca_cert_path,
            Some("/path/to/ca.pem".to_string())
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_development_config() {
        let config = SqlConfig::development("postgresql://localhost:5433/test");
        assert!(!config.is_tls_enabled());
        assert_eq!(config.pool.tls.mode, TlsMode::Disabled);
    }

    #[test]
    fn from_fundamental_config_maps_all_fields() {
        let settings = SqlSettings {
            url: DatabaseUrl::new("postgresql://custom:5432/testdb"),
            url_template: None,
            credentials: Default::default(),
            max_connections: 99,
            min_connections: 11,
            tls: TlsPosture::Enabled,
            tls_ca_cert_path: Some("/path/to/ca.pem".to_string()),
            acquire_timeout_secs: Some(45),
            statement_timeout_secs: Some(9),
            idle_timeout_secs: 900,
            ..Default::default()
        };

        let config = SqlConfig::from_fundamental_config(&settings, Environment::Production)
            .expect("no schema is configured, so this cannot refuse");

        assert!(config.pool.url.expose().contains("custom:5432/testdb"));
        assert_eq!(config.pool.max_connections, 99);
        assert_eq!(config.pool.min_connections, 11);
        assert_eq!(config.pool.acquire_timeout, Duration::from_secs(45));
        assert_eq!(config.pool.idle_timeout, Some(Duration::from_secs(900)));
        assert!(config.is_tls_enabled());
        assert_eq!(config.pool.tls.mode, TlsMode::VerifyFull);
        assert_eq!(
            config.pool.tls.ca_cert_path,
            Some("/path/to/ca.pem".to_string())
        );
    }

    #[test]
    fn a_configured_schema_reaches_the_pool() {
        let settings = SqlSettings {
            url: DatabaseUrl::new("postgresql://layer0@db:5433/layer0"),
            schema: Some("tenant_7".to_string()),
            acquire_timeout_secs: Some(2),
            ..Default::default()
        };

        let config = SqlConfig::from_fundamental_config(&settings, Environment::Production)
            .expect("a plain identifier is accepted");

        assert_eq!(config.default_schema.as_str(), "tenant_7");
    }

    #[test]
    fn no_configured_schema_is_the_wire_default() {
        let settings = SqlSettings {
            url: DatabaseUrl::new("postgresql://layer0@db:5433/layer0"),
            acquire_timeout_secs: Some(2),
            ..Default::default()
        };

        let config = SqlConfig::from_fundamental_config(&settings, Environment::Production)
            .expect("nothing to refuse");

        assert_eq!(config.default_schema.as_str(), SchemaName::DEFAULT);
    }

    #[test]
    fn a_schema_that_could_escape_a_statement_is_refused_at_boot() {
        for hostile in [
            "public; DROP TABLE users; --",
            "public.users",
            "1st",
            "with space",
        ] {
            let settings = SqlSettings {
                url: DatabaseUrl::new("postgresql://layer0@db:5433/layer0"),
                schema: Some(hostile.to_string()),
                acquire_timeout_secs: Some(2),
                ..Default::default()
            };

            let refused =
                SqlConfig::from_fundamental_config(&settings, Environment::Production).is_err();
            assert!(refused, "must refuse {hostile:?}");
        }
    }

    #[test]
    fn from_fundamental_config_development_no_tls() {
        let settings = SqlSettings {
            url: DatabaseUrl::new("postgresql://yugabyte@localhost:5433/dev"),
            tls: TlsPosture::Disabled,
            acquire_timeout_secs: Some(2),
            ..Default::default()
        };

        let config = SqlConfig::from_fundamental_config(&settings, Environment::Development)
            .expect("no schema is configured, so this cannot refuse");
        assert!(!config.is_tls_enabled());
        assert_eq!(config.pool.tls.mode, TlsMode::Disabled);
    }

    #[test]
    fn a_client_certificate_pair_configures_mutual_tls() {
        let settings = SqlSettings {
            url: DatabaseUrl::new("postgresql://layer1@yugabyte:5433/layer1"),
            tls: TlsPosture::Enabled,
            tls_ca_cert_path: Some("/certs/ca.crt".to_string()),
            tls_client_cert_path: Some("/certs/client.layer1.crt".to_string()),
            tls_client_key_path: Some("/certs/client.layer1.key".to_string()),
            acquire_timeout_secs: Some(2),
            ..Default::default()
        };

        let config = SqlConfig::from_fundamental_config(&settings, Environment::Production)
            .expect("no schema is configured, so this cannot refuse");

        assert_eq!(config.pool.tls.mode, TlsMode::VerifyFull);
        assert!(config.pool.tls.has_client_cert());
        assert_eq!(
            config.pool.tls.client_key_path.as_deref(),
            Some("/certs/client.layer1.key")
        );
    }

    #[test]
    fn a_client_certificate_without_its_key_is_not_applied() {
        let settings = SqlSettings {
            url: DatabaseUrl::new("postgresql://layer1@yugabyte:5433/layer1"),
            tls: TlsPosture::Enabled,
            tls_client_cert_path: Some("/certs/client.layer1.crt".to_string()),
            acquire_timeout_secs: Some(2),
            ..Default::default()
        };

        let config = SqlConfig::from_fundamental_config(&settings, Environment::Production)
            .expect("no schema is configured, so this cannot refuse");

        assert_eq!(config.pool.tls.client_cert_path, None);
        assert_eq!(config.pool.tls.client_key_path, None);
    }

    #[test]
    fn terminated_upstream_negotiates_nothing_here() {
        let settings = SqlSettings {
            url: DatabaseUrl::new("postgresql://layer1@localhost:5433/layer1"),
            tls: TlsPosture::TerminatedUpstream,
            tls_ca_cert_path: Some("/certs/ca.crt".to_string()),
            tls_client_cert_path: Some("/certs/client.layer1.crt".to_string()),
            tls_client_key_path: Some("/certs/client.layer1.key".to_string()),
            acquire_timeout_secs: Some(2),
            ..Default::default()
        };

        let config = SqlConfig::from_fundamental_config(&settings, Environment::Production)
            .expect("no schema is configured, so this cannot refuse");

        assert_eq!(config.pool.tls.mode, TlsMode::Disabled);
        assert!(!config.pool.tls.has_client_cert());
        assert!(!config.pool.tls.has_ca_cert());
    }

    #[test]
    fn a_pool_that_declares_no_acquire_bound_does_not_start() {
        let settings = SqlSettings {
            url: DatabaseUrl::new("postgresql://custom:5432/testdb"),
            acquire_timeout_secs: None,
            ..Default::default()
        };

        let refusal = SqlConfig::from_fundamental_config(&settings, Environment::Production)
            .expect_err("a pool with no declared wait must not build");

        assert!(
            format!("{refusal}").contains("acquire_timeout_secs"),
            "the refusal does not name the key an operator has to add: {refusal}"
        );
    }
}
