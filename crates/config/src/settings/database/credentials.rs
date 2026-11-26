//! Credential-source selection for database instances ([`CredentialSource`]).

use serde::{Deserialize, Serialize};

/// Where a database instance's credentials come from.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum CredentialSource {
    /// Credentials come from the static config. Default.
    #[default]
    Static,
    /// HashiCorp Vault mints short-lived, leased credentials; requires the Vault backend.
    VaultDynamic {
        /// The Vault database secrets-engine mount (e.g. `"database"`).
        mount: String,
        /// The role to request credentials for (e.g. `"app-sql"`).
        role: String,
    },
}

impl CredentialSource {
    /// Returns `true` if this source requires a configured secret manager.
    #[must_use]
    pub fn requires_secret_manager(&self) -> bool {
        matches!(self, Self::VaultDynamic { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_static() {
        let source = CredentialSource::default();
        assert!(matches!(source, CredentialSource::Static));
        assert!(!source.requires_secret_manager());
    }

    #[test]
    fn parses_vault_dynamic() {
        let toml = r#"
            source = "vault_dynamic"
            mount = "database"
            role = "app-sql"
        "#;
        let source: CredentialSource = toml::from_str(toml).unwrap();
        assert!(source.requires_secret_manager());
        match source {
            CredentialSource::VaultDynamic { mount, role } => {
                assert_eq!(mount, "database");
                assert_eq!(role, "app-sql");
            }
            CredentialSource::Static => panic!("expected VaultDynamic"),
        }
    }

    #[test]
    fn parses_static_from_tag() {
        let source: CredentialSource = toml::from_str(r#"source = "static""#).unwrap();
        assert!(matches!(source, CredentialSource::Static));
    }
}
