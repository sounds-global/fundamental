//! YugabyteDB YSQL connection settings.

use super::CredentialSource;
use crate::settings::TlsPosture;
use fundamental_core::secrets::DatabaseUrl;
use serde::{Deserialize, Serialize};

/// YugabyteDB YSQL configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SqlSettings {
    /// Database URL, loaded from a secret; without credentials for [`CredentialSource::VaultDynamic`].
    #[serde(skip)]
    pub url: DatabaseUrl,
    /// URL placeholder for the config file (may contain `${VAR}` references).
    #[serde(rename = "url")]
    pub url_template: Option<String>,
    /// Where this instance's credentials come from (static URL or Vault dynamic).
    #[serde(default)]
    pub credentials: CredentialSource,
    /// Maximum connection pool size.
    pub max_connections: u32,
    /// Minimum connection pool size.
    pub min_connections: u32,
    /// How this connection's transport security is arranged; accepts the legacy `tls_enabled`.
    #[serde(alias = "tls_enabled")]
    pub tls: TlsPosture,
    /// Path to the CA certificate (PEM) that signs the cluster's node certificates; raises to `VerifyFull`.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tls_ca_cert_path: Option<String>,
    /// The CA certificate inline, as PEM; takes precedence over the path.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tls_ca_cert_pem: Option<String>,
    /// Path to this client's certificate (PEM), for mutual TLS.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tls_client_cert_path: Option<String>,
    /// This client's certificate inline, as PEM.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tls_client_cert_pem: Option<String>,
    /// Path to this client's private key (PEM), for mutual TLS.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tls_client_key_path: Option<String>,
    /// This client's private key inline, as PEM; supply it through a secret reference.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tls_client_key_pem: Option<String>,
    /// How long a caller may wait for a connection from this pool, in seconds; required.
    ///
    /// `None` means nobody declared it, refused at boot. Keep it below the request timeout.
    #[serde(alias = "connect_timeout_secs")]
    pub acquire_timeout_secs: Option<u64>,
    /// How long a single statement may run before the engine aborts it; `None` is unlimited.
    pub statement_timeout_secs: Option<u64>,
    /// Idle connection timeout in seconds.
    pub idle_timeout_secs: u64,
    /// The schema every statement qualifies its tables with, and the `search_path`; default `public`.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub schema: Option<String>,
}

impl Default for SqlSettings {
    fn default() -> Self {
        Self {
            url: DatabaseUrl::default(),
            url_template: None,
            credentials: CredentialSource::default(),
            max_connections: 10,
            min_connections: 2,
            tls: TlsPosture::Disabled,
            tls_ca_cert_path: None,
            tls_ca_cert_pem: None,
            tls_client_cert_path: None,
            tls_client_cert_pem: None,
            tls_client_key_path: None,
            tls_client_key_pem: None,
            acquire_timeout_secs: None,
            statement_timeout_secs: None,
            idle_timeout_secs: 600,
            schema: None,
        }
    }
}
