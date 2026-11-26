//! Field-level encryption configuration ([`EncryptionSettings`]).

use std::collections::BTreeMap;

use fundamental_core::Environment;
use fundamental_core::secrets::SecretString;
use serde::{Deserialize, Serialize};

use crate::ConfigError;

/// Where this service sources its field-level encryption key material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyBackend {
    /// Keys come from environment variables (`{env_prefix}_{KEY_ID}`). Default.
    #[default]
    Env,
    /// Keys come from configuration values ([`EncryptionSettings::config_keys`]), kept out of the environment.
    Config,
    /// Keys are envelope-encrypted DEKs unwrapped by HashiCorp Vault Transit.
    Vault,
}

/// The AEAD algorithm a key is used with; mirrors `fundamental_encryption::EncryptionAlgorithm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum KeyAlgorithm {
    /// AES-256-GCM (default).
    #[default]
    #[serde(rename = "aes-256-gcm")]
    Aes256Gcm,
    /// ChaCha20-Poly1305.
    #[serde(rename = "chacha20-poly1305")]
    ChaCha20Poly1305,
}

/// A Vault-wrapped data encryption key (DEK), unwrapped at startup by Vault Transit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncryptedKeyRef {
    /// The key id used in the application (e.g. `"pii"`, `"default"`).
    pub id: String,
    /// The wrapped DEK as the raw Vault Transit ciphertext (`vault:v1:...`).
    pub encrypted_dek: String,
    /// The Vault Transit key name used to unwrap `encrypted_dek`.
    pub transit_key: String,
    /// The key version (for rotation). Defaults to 1.
    #[serde(default = "default_key_version")]
    pub version: u32,
    /// The AEAD algorithm this key is used with.
    #[serde(default)]
    pub algorithm: KeyAlgorithm,
    /// The 64-hex fingerprint the unwrapped material must have; required in production.
    #[serde(default)]
    pub fingerprint: Option<String>,
}

fn default_key_version() -> u32 {
    1
}

/// A data encryption key supplied as a configuration value, labelled `<purpose>:<version>:<64 hex>`.
///
/// The purpose comes from [`EncryptionSettings::with_purpose`], never from configuration.
///
/// ```toml
/// [encryption]
/// backend = "config"
/// config_keys = [{ id = "pii", material = "${file:/run/secrets/pii.key}" }]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigKeyRef {
    /// The key id used in the application (e.g. `"pii"`, `"default"`).
    pub id: String,
    /// The key material, `<purpose>:<version>:<64 hex>`. 32 bytes once decoded.
    pub material: SecretString,
    /// The key version (for rotation). Defaults to 1.
    #[serde(default = "default_key_version")]
    pub version: u32,
    /// The AEAD algorithm this key is used with.
    #[serde(default)]
    pub algorithm: KeyAlgorithm,
    /// The 64-hex fingerprint this key version's material must have, when declared.
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// Field-level encryption configuration: data only, the `KeyManager` is built where secrets live.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EncryptionSettings {
    /// Whether field-level encryption is enabled for this service.
    pub enabled: bool,

    /// Where key material is sourced from ([`KeyBackend::Env`] by default).
    pub backend: KeyBackend,

    /// The key ids this service uses, each read from `{env_prefix}_{KEY_ID}` ([`KeyBackend::Env`] mode).
    pub key_ids: Vec<String>,

    /// Environment variable prefix for env-sourced keys ([`KeyBackend::Env`] mode).
    pub env_prefix: String,

    /// Key material carried as configuration values ([`KeyBackend::Config`] mode).
    #[serde(default)]
    pub config_keys: Vec<ConfigKeyRef>,

    /// The purpose each configured key's material must be labelled for, by key id; never configured.
    #[serde(skip)]
    pub purposes: BTreeMap<String, String>,

    /// Vault-wrapped DEK references ([`KeyBackend::Vault`] mode).
    pub keys: Vec<EncryptedKeyRef>,
}

impl EncryptionSettings {
    /// Declares, from code, the purpose `key_id`'s material must be labelled for.
    #[must_use]
    pub fn with_purpose(mut self, key_id: impl Into<String>, purpose: impl Into<String>) -> Self {
        self.purposes.insert(key_id.into(), purpose.into());
        self
    }

    /// Refuses, in a protected `environment`, a Vault key that declares no fingerprint.
    ///
    /// # Errors
    /// [`ConfigError::InvalidValue`] (`CFG_004`) naming each undeclared key.
    pub fn require_declared_vault_keys(&self, environment: Environment) -> Result<(), ConfigError> {
        if self.backend != KeyBackend::Vault || !environment.is_protected() {
            return Ok(());
        }
        let undeclared: Vec<&str> = self
            .keys
            .iter()
            .filter(|key| {
                key.fingerprint
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
            })
            .map(|key| key.id.as_str())
            .collect();
        if undeclared.is_empty() {
            return Ok(());
        }
        Err(ConfigError::invalid_value(
            "encryption.keys",
            format!(
                "{} declare no fingerprint; {environment} loads only Vault keys it can verify",
                undeclared.join(", ")
            ),
        ))
    }
}

impl Default for EncryptionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: KeyBackend::Env,
            key_ids: vec!["default".to_string()],
            env_prefix: "FUNDAMENTAL_KEY".to_string(),
            config_keys: Vec::new(),
            purposes: BTreeMap::new(),
            keys: Vec::new(),
        }
    }
}

#[cfg(test)]
mod purpose_tests {
    use super::{ConfigKeyRef, EncryptionSettings};

    #[test]
    fn a_purpose_written_in_configuration_fails_the_load() {
        let error = toml::from_str::<ConfigKeyRef>(
            "id = \"pii\"\npurpose = \"app.at-rest.v1\"\nmaterial = \"m\"",
        )
        .expect_err("purpose is not a configuration field");
        assert!(error.to_string().contains("purpose"), "{error}");
    }

    #[test]
    fn purposes_are_never_read_from_configuration() {
        let settings = toml::from_str::<EncryptionSettings>(
            "backend = \"config\"\n[purposes]\npii = \"app.at-rest.v1\"",
        )
        .expect("an unknown table on the settings is ignored");
        assert!(settings.purposes.is_empty());
        let declared = settings.with_purpose("pii", "app.at-rest.v1");
        assert_eq!(
            declared.purposes.get("pii").map(String::as_str),
            Some("app.at-rest.v1")
        );
    }
}

#[cfg(test)]
mod vault_key_tests {
    use fundamental_core::Environment;
    use fundamental_core::Error as _;

    use super::{EncryptedKeyRef, EncryptionSettings, KeyBackend};

    fn vault_keys(fingerprints: &[Option<&str>]) -> EncryptionSettings {
        EncryptionSettings {
            backend: KeyBackend::Vault,
            keys: fingerprints
                .iter()
                .enumerate()
                .map(|(index, fingerprint)| EncryptedKeyRef {
                    id: format!("key-{index}"),
                    encrypted_dek: "vault:v1:x".to_owned(),
                    transit_key: "t".to_owned(),
                    version: 1,
                    algorithm: super::KeyAlgorithm::default(),
                    fingerprint: fingerprint.map(str::to_owned),
                })
                .collect(),
            ..EncryptionSettings::default()
        }
    }

    #[test]
    fn a_misspelt_fingerprint_fails_the_load_rather_than_reading_as_none() {
        let error = toml::from_str::<EncryptedKeyRef>(
            "id = \"pii\"\nencrypted_dek = \"vault:v1:x\"\ntransit_key = \"t\"\nfingerprnt = \"ab\"",
        )
        .expect_err("an unknown field is refused");
        assert!(error.to_string().contains("fingerprnt"), "{error}");
    }

    #[test]
    fn a_protected_environment_refuses_an_undeclared_vault_key() {
        let settings = vault_keys(&[Some("ab"), None, Some("  ")]);
        let refused = settings
            .require_declared_vault_keys(Environment::Production)
            .expect_err("an undeclared key is refused");
        assert_eq!(refused.error_code(), "CFG_004");
        let reason = refused.to_string();
        assert!(
            reason.contains("key-1") && reason.contains("key-2"),
            "{reason}"
        );
        assert!(!reason.contains("key-0"), "{reason}");
    }

    #[test]
    fn development_and_declared_keys_and_other_backends_load() {
        let undeclared = vault_keys(&[None]);
        for unprotected in [Environment::Development, Environment::Staging] {
            assert!(undeclared.require_declared_vault_keys(unprotected).is_ok());
        }
        assert!(
            vault_keys(&[Some("ab")])
                .require_declared_vault_keys(Environment::Production)
                .is_ok()
        );
        let config_backend = EncryptionSettings {
            backend: KeyBackend::Config,
            ..undeclared
        };
        assert!(
            config_backend
                .require_declared_vault_keys(Environment::Production)
                .is_ok()
        );
    }
}
