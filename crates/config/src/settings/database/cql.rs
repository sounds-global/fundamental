//! YugabyteDB YCQL connection settings.

use super::CredentialSource;
use crate::settings::TlsPosture;
use fundamental_core::secrets::InMemorySecretString;
use serde::{Deserialize, Serialize};

/// YugabyteDB YCQL configuration; an unknown key is refused rather than silently dropped.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CqlSettings {
    /// Cluster hosts.
    pub hosts: Vec<String>,
    /// Keyspace name.
    pub keyspace: String,
    /// How this connection's transport security is arranged; accepts the legacy `tls_enabled`.
    #[serde(alias = "tls_enabled")]
    pub tls: TlsPosture,
    /// Path to the CA certificate (PEM) that signs the cluster's node certificates.
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
    /// The name node certificates are verified against, instead of the address dialled.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tls_server_name: Option<String>,
    /// Rewrites of the addresses nodes advertise, as `from=to` pairs; a target may be a `host:port`.
    pub address_translation: Vec<String>,
    /// Username; ignored for [`CredentialSource::VaultDynamic`], which leases one.
    pub username: Option<String>,
    /// Password, redacted when serialized; ignored for [`CredentialSource::VaultDynamic`].
    #[serde(default)]
    pub password: InMemorySecretString,
    /// Where this instance's credentials come from (static or Vault dynamic).
    #[serde(default)]
    pub credentials: CredentialSource,
    /// Connection pool size per host.
    pub connections_per_host: u32,
    /// Whether requests are routed by token; turn it off on YCQL, which lacks the schema tables it reads.
    pub token_aware_routing: bool,
}

impl Default for CqlSettings {
    fn default() -> Self {
        Self {
            hosts: vec!["localhost:9042".to_string()],
            keyspace: "app".to_string(),
            tls: TlsPosture::Disabled,
            tls_ca_cert_path: None,
            tls_ca_cert_pem: None,
            tls_client_cert_path: None,
            tls_client_cert_pem: None,
            tls_client_key_path: None,
            tls_client_key_pem: None,
            tls_server_name: None,
            address_translation: Vec::new(),
            username: None,
            password: InMemorySecretString::default(), // absent
            credentials: CredentialSource::default(),
            connections_per_host: 1,
            token_aware_routing: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CqlSettings;

    #[test]
    fn a_key_this_engine_cannot_read_refuses_rather_than_vanishing() {
        let refusal = toml::from_str::<CqlSettings>(
            "keyspace = \"layer0\"\nconnections_per_host = 8\nmax_connections = 8\n",
        )
        .expect_err("a key this type has no field for must not deserialize");
        assert!(
            format!("{refusal}").contains("max_connections"),
            "{refusal}"
        );

        let accepted: CqlSettings =
            toml::from_str("keyspace = \"layer0\"\nconnections_per_host = 8\n")
                .expect("the key this engine does read is accepted");
        assert_eq!(accepted.connections_per_host, 8);
    }

    #[test]
    fn a_connection_can_turn_token_aware_routing_off() {
        let off: CqlSettings =
            toml::from_str("keyspace = \"layer0\"\ntoken_aware_routing = false\n")
                .expect("the routing key is read");
        assert!(!off.token_aware_routing);

        let absent: CqlSettings =
            toml::from_str("keyspace = \"layer0\"\n").expect("a section without it");
        assert!(absent.token_aware_routing, "absent keeps routing on");
    }
}
