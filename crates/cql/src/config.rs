//! YCQL connection configuration.

use std::time::Duration;

use fundamental_config::settings::TlsPosture;
use fundamental_core::secrets::SecretString;
use fundamental_db::{DDL_TIME_BUDGET, TlsConfig, TlsMode};

use crate::{CircuitBreakerConfig, Consistency, RetryConfig};

/// How long a schema statement may take before the client abandons it, unless configured.
pub const DEFAULT_DDL_TIMEOUT: Duration = DDL_TIME_BUDGET;

/// Configuration for YCQL connections.
#[derive(Debug, Clone)]
pub struct CqlConfig {
    /// Contact points (seed nodes) for initial connection.
    pub contact_points: Vec<String>,

    /// Default keyspace to use.
    pub default_keyspace: Option<String>,

    /// Local datacenter name for token-aware routing.
    pub local_datacenter: Option<String>,

    /// Default consistency level for reads.
    pub read_consistency: Consistency,

    /// Default consistency level for writes.
    pub write_consistency: Consistency,

    /// Connection timeout.
    pub connect_timeout: Duration,

    /// Request timeout.
    pub request_timeout: Duration,

    /// How long a schema statement may take before the client abandons it.
    ///
    /// Separate from [`request_timeout`](Self::request_timeout): DDL waits on a master. Never
    /// retried, because a CQL timeout does not say whether the statement ran.
    pub ddl_timeout: Duration,

    /// Number of connections per host.
    pub connections_per_host: usize,

    /// Enable compression.
    pub compression: bool,

    /// Application name for identification.
    pub application_name: Option<String>,

    /// Enable token-aware routing.
    ///
    /// It makes the driver fetch schema metadata; set it `false` on YugabyteDB, whose YCQL wire
    /// lacks the `system_schema.scylla_*` tables the fetch reads. Defaults to `true`.
    pub token_aware_routing: bool,

    /// Prepared statement cache size.
    pub prepared_cache_size: usize,

    /// TLS configuration for secure connections.
    pub tls: TlsConfig,

    /// Username for authentication.
    ///
    /// Used with `password`; also read from `CQL_USERNAME`. A secret, as Vault-dynamic credentials
    /// make it one.
    pub username: Option<SecretString>,

    /// Password for authentication.
    ///
    /// Used with `username`; also read from `CQL_PASSWORD`.
    pub password: Option<SecretString>,

    /// Circuit breaker configuration; when set, the pool wraps operations in one.
    pub circuit_breaker: Option<CircuitBreakerConfig>,

    /// Retry configuration; when set, operations are retried on transient failures.
    pub retry: Option<RetryConfig>,

    /// Startup retry configuration; when set, the initial connection is retried.
    pub startup_retry: Option<RetryConfig>,

    /// How long an operation may take before a warning is logged; `None` disables it.
    pub leak_detection_threshold: Option<Duration>,

    /// Rewrites of the addresses nodes advertise, as `from=to` pairs
    /// (`10.0.0.7:9142=127.0.0.1:9142`).
    ///
    /// Kept as written: a malformed pair is reported when the session is built.
    pub address_translation: Vec<String>,
}

impl CqlConfig {
    /// Creates a new configuration with the given contact points.
    ///
    /// TLS is enabled by default (Required mode).
    ///
    /// # Example
    ///
    /// ```rust
    /// use fundamental_cql::CqlConfig;
    ///
    /// let config = CqlConfig::new(["ycql1:9042", "ycql2:9042"]);
    /// ```
    pub fn new<I, S>(contact_points: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            contact_points: contact_points.into_iter().map(Into::into).collect(),
            default_keyspace: None,
            local_datacenter: None,
            read_consistency: Consistency::default_read(),
            write_consistency: Consistency::default_write(),
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(12),
            ddl_timeout: DEFAULT_DDL_TIMEOUT,
            connections_per_host: 1,
            compression: true,
            application_name: None,
            token_aware_routing: true,
            prepared_cache_size: 1000,
            tls: TlsConfig::default(),
            username: None,
            password: None,
            circuit_breaker: None,
            retry: None,
            startup_retry: None,
            leak_detection_threshold: Some(Duration::from_secs(30)),
            address_translation: Vec::new(),
        }
    }

    /// Creates a configuration for development without TLS.
    #[cfg(debug_assertions)]
    pub fn development<I, S>(contact_points: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            tls: TlsConfig::disabled(),
            ..Self::new(contact_points)
        }
    }

    /// Creates configuration from environment variables.
    ///
    /// Reads from:
    /// - `CQL_HOSTS` - Comma-separated list of contact points
    /// - `CQL_KEYSPACE` - Default keyspace
    /// - `CQL_DATACENTER` - Local datacenter
    /// - `CQL_CA_CERT` - CA certificate path for TLS verification
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn from_env() -> Result<Self, std::env::VarError> {
        let hosts = std::env::var("CQL_HOSTS")?;
        let contact_points: Vec<String> = hosts.split(',').map(|s| s.trim().to_string()).collect();

        let mut config = Self::new(contact_points);

        if let Ok(keyspace) = std::env::var("CQL_KEYSPACE") {
            config.default_keyspace = Some(keyspace);
        }

        if let Ok(dc) = std::env::var("CQL_DATACENTER") {
            config.local_datacenter = Some(dc);
        }

        if let Ok(ca_cert) = std::env::var("CQL_CA_CERT") {
            config = config.tls_ca_cert(ca_cert);
        }

        if let Ok(username) = std::env::var("CQL_USERNAME") {
            config.username = Some(SecretString::new(username));
        }
        if let Ok(password) = std::env::var("CQL_PASSWORD") {
            config.password = Some(SecretString::new(password));
        }

        Ok(config)
    }

    /// Builds a configuration from `fundamental_config` settings.
    ///
    /// The keyspace comes from the connection, not the model, so a model is portable across
    /// keyspaces.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn from_settings(
        settings: &fundamental_config::settings::CqlSettings,
        environment: fundamental_core::Environment,
    ) -> Self {
        let mut config = Self::new(settings.hosts.iter().cloned());

        config.default_keyspace = Some(settings.keyspace.clone());
        config.address_translation = settings.address_translation.clone();
        config.connections_per_host = settings.connections_per_host.max(1) as usize;
        config.token_aware_routing = settings.token_aware_routing;
        config.username = settings.username.clone().map(SecretString::new);
        config.password = settings
            .password
            .expose()
            .map(|p| SecretString::new(p.clone()));

        // TLS from the declared posture. `TerminatedUpstream` leaves the handshake to a declared
        // proxy; `Disabled` still gets `Required` in a release build, since `validate()` was
        // bypassed.
        config.tls = match settings.tls {
            TlsPosture::Enabled => TlsConfig::new().mode(TlsMode::Required),
            TlsPosture::TerminatedUpstream => TlsConfig::terminated_upstream(),
            TlsPosture::Disabled => {
                #[cfg(debug_assertions)]
                {
                    if environment.allows_insecure_tls() {
                        TlsConfig::disabled()
                    } else {
                        TlsConfig::new().mode(TlsMode::Required)
                    }
                }
                #[cfg(not(debug_assertions))]
                {
                    let _ = environment;
                    TlsConfig::new().mode(TlsMode::Required)
                }
            }
        };

        // A configured anchor upgrades `Required` to `VerifyFull`; a disabled dev connection has
        // nothing to verify.
        if config.tls.mode.is_enabled() {
            // Inline PEM wins over a path, which may not exist in this container.
            if let Some(ca_cert_pem) = settings.tls_ca_cert_pem.as_deref() {
                config.tls = config
                    .tls
                    .ca_cert_pem(ca_cert_pem)
                    .mode(TlsMode::VerifyFull);
            } else if let Some(ca_cert_path) = settings.tls_ca_cert_path.as_deref() {
                config = config.tls_ca_cert(ca_cert_path);
            }

            // Only meaningful with an anchor, so set inside the enabled-TLS block.
            if let Some(server_name) = settings.tls_server_name.as_deref() {
                config.tls = config.tls.server_name(server_name);
            }

            // Mutual TLS: both halves or neither; half-configured mTLS fails at connect like a
            // network fault.
            match (
                settings.tls_client_cert_pem.as_deref(),
                settings.tls_client_key_pem.as_deref(),
            ) {
                (Some(cert), Some(key)) => config.tls = config.tls.client_cert_pem(cert, key),
                _ => {
                    if let (Some(cert), Some(key)) = (
                        settings.tls_client_cert_path.as_deref(),
                        settings.tls_client_key_path.as_deref(),
                    ) {
                        config.tls = config.tls.client_cert(cert, key);
                    }
                }
            }
        }

        config
    }

    /// Sets the default keyspace.
    pub fn keyspace(mut self, keyspace: impl Into<String>) -> Self {
        self.default_keyspace = Some(keyspace.into());
        self
    }

    /// Sets the local datacenter.
    pub fn local_datacenter(mut self, dc: impl Into<String>) -> Self {
        self.local_datacenter = Some(dc.into());
        self
    }

    /// Sets the default read consistency.
    pub fn read_consistency(mut self, consistency: Consistency) -> Self {
        self.read_consistency = consistency;
        self
    }

    /// Sets the default write consistency.
    pub fn write_consistency(mut self, consistency: Consistency) -> Self {
        self.write_consistency = consistency;
        self
    }

    /// Sets the default consistency for both reads and writes.
    pub fn default_consistency(mut self, consistency: Consistency) -> Self {
        self.read_consistency = consistency;
        self.write_consistency = consistency;
        self
    }

    /// Sets the connection timeout.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the request timeout.
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets the deadline for a schema statement.
    pub fn ddl_timeout(mut self, timeout: Duration) -> Self {
        self.ddl_timeout = timeout;
        self
    }

    /// Sets the number of connections per host.
    pub fn connections_per_host(mut self, count: usize) -> Self {
        self.connections_per_host = count;
        self
    }

    /// Enables or disables compression.
    pub fn compression(mut self, enable: bool) -> Self {
        self.compression = enable;
        self
    }

    /// Enables or disables token-aware routing.
    pub fn token_aware_routing(mut self, enable: bool) -> Self {
        self.token_aware_routing = enable;
        self
    }

    /// Sets the application name.
    pub fn application_name(mut self, name: impl Into<String>) -> Self {
        self.application_name = Some(name.into());
        self
    }

    /// Sets the prepared statement cache size.
    pub fn prepared_cache_size(mut self, size: usize) -> Self {
        self.prepared_cache_size = size;
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
    ///
    /// This also enables `VerifyFull` TLS mode.
    pub fn tls_ca_cert(mut self, path: impl Into<String>) -> Self {
        self.tls.ca_cert_path = Some(path.into());
        self.tls.mode = TlsMode::VerifyFull;
        self
    }

    /// Returns whether TLS is enabled.
    pub fn is_tls_enabled(&self) -> bool {
        self.tls.mode.is_enabled()
    }

    /// Sets the authentication credentials.
    ///
    /// Both username and password must be provided for authentication to be used.
    pub fn with_credentials(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.username = Some(SecretString::new(username.into()));
        self.password = Some(SecretString::new(password.into()));
        self
    }

    /// Returns whether authentication is configured.
    pub fn has_credentials(&self) -> bool {
        self.username.is_some() && self.password.is_some()
    }

    /// Sets the circuit breaker configuration.
    pub fn with_circuit_breaker(mut self, config: CircuitBreakerConfig) -> Self {
        self.circuit_breaker = Some(config);
        self
    }

    /// Sets the retry configuration.
    pub fn with_retry(mut self, config: RetryConfig) -> Self {
        self.retry = Some(config);
        self
    }

    /// Sets the startup retry configuration.
    pub fn with_startup_retry(mut self, config: RetryConfig) -> Self {
        self.startup_retry = Some(config);
        self
    }

    /// Returns whether a circuit breaker is configured.
    pub fn has_circuit_breaker(&self) -> bool {
        self.circuit_breaker.is_some()
    }

    /// Returns whether retry is configured.
    pub fn has_retry(&self) -> bool {
        self.retry.is_some()
    }

    /// Sets the operation leak detection threshold.
    ///
    /// An operation taking longer logs a warning; `None` disables it.
    pub fn leak_detection_threshold(mut self, threshold: Option<Duration>) -> Self {
        self.leak_detection_threshold = threshold;
        self
    }
}

impl Default for CqlConfig {
    fn default() -> Self {
        Self::new(["127.0.0.1:9042"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = CqlConfig::new(["host1:9042", "host2:9042"])
            .keyspace("events")
            .local_datacenter("dc1")
            .default_consistency(Consistency::Quorum)
            .token_aware_routing(true);

        assert_eq!(config.contact_points.len(), 2);
        assert_eq!(config.default_keyspace, Some("events".into()));
        assert_eq!(config.local_datacenter, Some("dc1".into()));
        assert_eq!(config.read_consistency, Consistency::Quorum);
        assert!(config.token_aware_routing);
    }

    #[test]
    fn a_configured_ca_anchor_is_verified_against() {
        let settings = fundamental_config::settings::CqlSettings {
            tls: TlsPosture::Enabled,
            tls_ca_cert_path: Some("/etc/ssl/certs/cluster-ca.pem".to_string()),
            ..Default::default()
        };

        let config = CqlConfig::from_settings(&settings, fundamental_core::Environment::Production);

        assert_eq!(config.tls.mode, TlsMode::VerifyFull);
        assert!(
            config.tls.mode.verifies_certificate(),
            "naming a CA must verify against it, not merely encrypt"
        );
        assert_eq!(
            config.tls.ca_cert_path.as_deref(),
            Some("/etc/ssl/certs/cluster-ca.pem")
        );
    }

    #[test]
    fn tls_without_an_anchor_stays_required_but_unverified() {
        let settings = fundamental_config::settings::CqlSettings {
            tls: TlsPosture::Enabled,
            ..Default::default()
        };

        let config = CqlConfig::from_settings(&settings, fundamental_core::Environment::Production);

        assert_eq!(config.tls.mode, TlsMode::Required);
        assert!(!config.tls.mode.verifies_certificate());
    }

    #[test]
    fn an_inline_ca_pem_is_verified_against_and_beats_a_path() {
        let settings = fundamental_config::settings::CqlSettings {
            tls: TlsPosture::Enabled,
            tls_ca_cert_path: Some("/etc/ssl/certs/stale.pem".to_string()),
            tls_ca_cert_pem: Some("-----BEGIN CERTIFICATE-----inline".to_string()),
            ..Default::default()
        };

        let config = CqlConfig::from_settings(&settings, fundamental_core::Environment::Production);

        assert_eq!(config.tls.mode, TlsMode::VerifyFull);
        assert_eq!(
            config.tls.ca_cert_pem.as_deref(),
            Some("-----BEGIN CERTIFICATE-----inline")
        );
        assert_eq!(
            config.tls.ca_cert_path, None,
            "a value supplied directly is the more specific instruction than a filename"
        );
    }

    #[test]
    fn a_client_certificate_pair_configures_mutual_tls() {
        let settings = fundamental_config::settings::CqlSettings {
            tls: TlsPosture::Enabled,
            tls_client_cert_path: Some("/etc/ssl/certs/client.pem".to_string()),
            tls_client_key_path: Some("/etc/ssl/private/client-key.pem".to_string()),
            ..Default::default()
        };

        let config = CqlConfig::from_settings(&settings, fundamental_core::Environment::Production);

        assert_eq!(
            config.tls.client_cert_path.as_deref(),
            Some("/etc/ssl/certs/client.pem")
        );
        assert_eq!(
            config.tls.client_key_path.as_deref(),
            Some("/etc/ssl/private/client-key.pem")
        );
    }

    #[test]
    fn a_client_certificate_without_its_key_is_not_applied() {
        let settings = fundamental_config::settings::CqlSettings {
            tls: TlsPosture::Enabled,
            tls_client_cert_path: Some("/etc/ssl/certs/client.pem".to_string()),
            ..Default::default()
        };

        let config = CqlConfig::from_settings(&settings, fundamental_core::Environment::Production);

        assert_eq!(config.tls.client_cert_path, None);
        assert_eq!(config.tls.client_key_path, None);
    }

    #[test]
    fn terminated_upstream_leaves_the_handshake_to_the_sidecar() {
        let settings = fundamental_config::settings::CqlSettings {
            tls: TlsPosture::TerminatedUpstream,
            ..Default::default()
        };

        let config = CqlConfig::from_settings(&settings, fundamental_core::Environment::Production);

        assert_eq!(config.tls.mode, TlsMode::Disabled);
        assert!(!config.is_tls_enabled());
    }

    #[test]
    fn a_disabled_posture_still_gets_tls_in_a_deployed_environment() {
        let settings = fundamental_config::settings::CqlSettings {
            tls: TlsPosture::Disabled,
            ..Default::default()
        };

        let config = CqlConfig::from_settings(&settings, fundamental_core::Environment::Production);

        assert_eq!(config.tls.mode, TlsMode::Required);
    }

    #[test]
    fn credentials_are_redacted_in_debug() {
        let config = CqlConfig::new(["host1:9042"]).with_credentials("leased_user", "leased_pw");
        assert!(config.has_credentials());
        let rendered = format!("{config:?}");
        assert!(
            !rendered.contains("leased_user"),
            "Debug must not leak the (possibly leased) username: {rendered}"
        );
        assert!(
            !rendered.contains("leased_pw"),
            "Debug must not leak the password: {rendered}"
        );
    }

    #[test]
    fn the_settings_routing_reaches_the_config() {
        let off = fundamental_config::settings::CqlSettings {
            token_aware_routing: false,
            ..Default::default()
        };
        let on = fundamental_config::settings::CqlSettings::default();

        assert!(
            !CqlConfig::from_settings(&off, fundamental_core::Environment::Production)
                .token_aware_routing
        );
        assert!(
            CqlConfig::from_settings(&on, fundamental_core::Environment::Production)
                .token_aware_routing
        );
    }

    #[test]
    fn credentials_are_readable_for_connecting() {
        let config = CqlConfig::new(["host1:9042"]).with_credentials("u", "p");
        assert_eq!(
            config.username.as_ref().map(|s| s.expose().as_str()),
            Some("u")
        );
        assert_eq!(
            config.password.as_ref().map(|s| s.expose().as_str()),
            Some("p")
        );
    }

    #[test]
    fn test_default_config() {
        let config = CqlConfig::default();
        assert_eq!(config.contact_points, vec!["127.0.0.1:9042"]);
        assert!(config.token_aware_routing);
        assert!(config.compression);
        assert_eq!(
            config.leak_detection_threshold,
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn test_tls_default_enabled() {
        let config = CqlConfig::new(["localhost:9042"]);
        assert!(config.is_tls_enabled());
        assert_eq!(config.tls.mode, TlsMode::Required);
    }

    #[test]
    fn test_tls_ca_cert() {
        let config = CqlConfig::new(["localhost:9042"]).tls_ca_cert("/path/to/ca.pem");

        assert!(config.is_tls_enabled());
        assert_eq!(config.tls.mode, TlsMode::VerifyFull);
        assert_eq!(config.tls.ca_cert_path, Some("/path/to/ca.pem".to_string()));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_development_config() {
        let config = CqlConfig::development(["localhost:9042"]);
        assert!(!config.is_tls_enabled());
        assert_eq!(config.tls.mode, TlsMode::Disabled);
    }

    #[test]
    fn test_leak_detection_threshold() {
        let config = CqlConfig::new(["localhost:9042"])
            .leak_detection_threshold(Some(Duration::from_secs(60)));

        assert_eq!(
            config.leak_detection_threshold,
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn test_leak_detection_disabled() {
        let config = CqlConfig::new(["localhost:9042"]).leak_detection_threshold(None);

        assert_eq!(config.leak_detection_threshold, None);
    }
}
