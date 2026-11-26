//! HashiCorp Vault settings ([`VaultSettings`], [`VaultAuth`]).

use fundamental_core::secrets::{InMemorySecret, InMemorySecretString};
use serde::{Deserialize, Serialize};

/// How the service authenticates to HashiCorp Vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum VaultAuth {
    /// A directly-supplied client token (e.g. `${VAULT_TOKEN}`).
    Token {
        /// The Vault client token.
        token: InMemorySecretString,
    },
    /// AppRole login (`role_id` + `secret_id`).
    AppRole {
        /// The AppRole role id.
        role_id: String,
        /// The AppRole secret id (e.g. `${VAULT_SECRET_ID}`).
        secret_id: InMemorySecretString,
    },
    /// Kubernetes service-account login.
    Kubernetes {
        /// The Vault role bound to the service account.
        role: String,
        /// Path to the projected service-account JWT.
        #[serde(default = "default_jwt_path")]
        jwt_path: String,
    },
    /// TLS certificate login (`auth/cert/login`) with the client certificate the service holds.
    Cert {
        /// The certificate role to log in against, when the method holds several.
        #[serde(default)]
        role: Option<String>,
        /// Path to the PEM client certificate presented in the TLS handshake.
        client_cert_path: String,
        /// Path to the PEM private key of that certificate.
        client_key_path: String,
    },
}

fn default_jwt_path() -> String {
    "/var/run/secrets/kubernetes.io/serviceaccount/token".to_string()
}

impl Default for VaultAuth {
    fn default() -> Self {
        // Absent, not an empty string, which would pass for a real blank token.
        Self::Token {
            token: InMemorySecret::absent(),
        }
    }
}

/// HashiCorp Vault connection configuration, shared by database credentials and encryption keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultSettings {
    /// Whether the Vault backend is enabled.
    pub enabled: bool,
    /// The Vault server address (e.g. `${VAULT_ADDR}` → `https://vault:8200`).
    pub address: String,
    /// Optional namespace every request is sent in (Vault Enterprise, OpenBao); empty means none.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub namespace: Option<String>,
    /// How to authenticate to Vault.
    pub auth: VaultAuth,
    /// Path to a custom CA certificate (PEM) for the Vault TLS connection.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub ca_cert_path: Option<String>,
    /// Skip TLS verification (development only — never set in production).
    pub tls_skip_verify: bool,
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            address: "http://127.0.0.1:8200".to_string(),
            namespace: None,
            auth: VaultAuth::default(),
            ca_cert_path: None,
            tls_skip_verify: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled_with_token_auth() {
        let settings = VaultSettings::default();
        assert!(!settings.enabled);
        assert!(matches!(settings.auth, VaultAuth::Token { .. }));
    }

    #[test]
    fn an_empty_namespace_is_no_namespace() {
        let settings: VaultSettings = toml::from_str("namespace = \"\"").unwrap();
        assert_eq!(settings.namespace, None);
        let settings: VaultSettings = toml::from_str("namespace = \"sandbox\"").unwrap();
        assert_eq!(settings.namespace.as_deref(), Some("sandbox"));
    }

    #[test]
    fn parses_approle_auth() {
        let toml = r#"
            enabled = true
            address = "https://vault.internal:8200"
            namespace = "team-a"

            [auth]
            method = "app_role"
            role_id = "rid-123"
            secret_id = "sid-456"
        "#;
        let settings: VaultSettings = toml::from_str(toml).unwrap();
        assert!(settings.enabled);
        assert_eq!(settings.address, "https://vault.internal:8200");
        assert_eq!(settings.namespace.as_deref(), Some("team-a"));
        match settings.auth {
            VaultAuth::AppRole { role_id, secret_id } => {
                assert_eq!(role_id, "rid-123");
                assert_eq!(secret_id.expose().map(String::as_str), Some("sid-456"));
            }
            other => panic!("expected AppRole, got {other:?}"),
        }
    }

    #[test]
    fn serializing_settings_never_leaks_the_secret() {
        let settings = VaultSettings {
            enabled: true,
            address: "https://vault:8200".to_string(),
            auth: VaultAuth::AppRole {
                role_id: "rid-123".to_string(),
                secret_id: InMemorySecret::new("s.hvs.LIVE-SECRET-ID".to_string()),
            },
            ..VaultSettings::default()
        };

        let toml = toml::to_string(&settings).expect("serialize settings to toml");
        assert!(
            !toml.contains("LIVE-SECRET-ID"),
            "secret leaked in TOML: {toml}"
        );
        assert!(toml.contains("[REDACTED]"));

        if let VaultAuth::AppRole { secret_id, .. } = &settings.auth {
            assert_eq!(
                secret_id.expose().map(String::as_str),
                Some("s.hvs.LIVE-SECRET-ID")
            );
        } else {
            panic!("expected AppRole");
        }
    }

    #[test]
    fn parses_kubernetes_auth_with_default_jwt_path() {
        let toml = r#"
            [auth]
            method = "kubernetes"
            role = "app"
        "#;
        let settings: VaultSettings = toml::from_str(toml).unwrap();
        match settings.auth {
            VaultAuth::Kubernetes { role, jwt_path } => {
                assert_eq!(role, "app");
                assert_eq!(jwt_path, default_jwt_path());
            }
            other => panic!("expected Kubernetes, got {other:?}"),
        }
    }

    #[test]
    fn parses_cert_auth_with_its_role_and_key_pair() {
        let toml = r#"
            [auth]
            method = "cert"
            role = "ops-ca"
            client_cert_path = "/certs/client.ops_ca.crt"
            client_key_path = "/certs/client.ops_ca.key"
        "#;
        let settings: VaultSettings = toml::from_str(toml).expect("parses");
        match settings.auth {
            VaultAuth::Cert {
                role,
                client_cert_path,
                client_key_path,
            } => {
                assert_eq!(role.as_deref(), Some("ops-ca"));
                assert_eq!(client_cert_path, "/certs/client.ops_ca.crt");
                assert_eq!(client_key_path, "/certs/client.ops_ca.key");
            }
            other => panic!("expected Cert, got {other:?}"),
        }
    }
}
