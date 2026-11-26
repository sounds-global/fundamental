//! Strongly-typed configuration settings, rooted at [`FundamentalConfig`].

mod api;
mod authz;
mod config;
mod database;
mod domain;
mod empty_as_none;
mod encryption;
mod i18n;
mod is_secret_path;
mod is_transit_key_name;
mod is_transit_mount;
mod message;
mod names_loopback;
mod project;
mod scalar_or_string;
mod secret_ref;
mod server;
mod surface;
mod tls_posture;
mod vault;
mod vault_kv_ref;

pub use api::ApiSettings;
pub use authz::{AuthzConsistency, AuthzSettings};
pub use config::FundamentalConfig;
pub use empty_as_none::empty_as_none;
pub use encryption::{ConfigKeyRef, EncryptedKeyRef, EncryptionSettings, KeyAlgorithm, KeyBackend};
pub use i18n::TranslationSettings;
pub use is_secret_path::is_secret_path;
pub use is_transit_key_name::is_transit_key_name;
pub use is_transit_mount::is_transit_mount;
pub(crate) use names_loopback::names_loopback;
pub use scalar_or_string::{optional_scalar_or_string, scalar_or_string};
pub use secret_ref::SecretRef;
pub use server::ServerSettings;
pub use tls_posture::TlsPosture;
pub use vault::{VaultAuth, VaultSettings};
pub use vault_kv_ref::VaultKvRef;

pub use database::*;
pub use domain::*;
pub use message::*;
pub use project::*;
pub use surface::*;

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Environment;
    use serde::Deserialize;

    #[test]
    fn test_fundamental_config_default() {
        let config = FundamentalConfig::default();
        assert!(config.environment.is_development());
        assert_eq!(config.server.port, 11001);
        assert_eq!(config.api.port, 16001);
    }

    #[test]
    fn test_server_settings_default() {
        let settings = ServerSettings::default();
        assert_eq!(settings.host, "127.0.0.1");
        assert_eq!(settings.port, 11001);
        assert_eq!(settings.tls, TlsPosture::Disabled);
    }

    #[test]
    fn test_custom_settings_get() {
        let mut custom = CustomSettings::new();
        custom.set("app.name", toml::Value::String("test".to_string()));
        custom.set("app.debug", toml::Value::Boolean(true));
        custom.set("app.port", toml::Value::Integer(8080));

        assert_eq!(custom.get_str("app.name"), Some("test"));
        assert_eq!(custom.get_bool("app.debug"), Some(true));
        assert_eq!(custom.get_int("app.port"), Some(8080));
        assert_eq!(custom.get_str("missing"), None);
    }

    #[test]
    fn test_custom_settings_section() {
        let mut custom = CustomSettings::new();
        custom.set(
            "payment.provider",
            toml::Value::String("stripe".to_string()),
        );
        custom.set("payment.sandbox", toml::Value::Boolean(true));

        #[derive(Debug, Deserialize, PartialEq)]
        struct PaymentConfig {
            provider: String,
            sandbox: bool,
        }

        let payment: PaymentConfig = custom.section("payment").unwrap();
        assert_eq!(payment.provider, "stripe");
        assert!(payment.sandbox);
    }

    #[test]
    fn test_custom_settings_merge() {
        let mut base = CustomSettings::new();
        base.set("key1", toml::Value::String("value1".to_string()));
        base.set("key2", toml::Value::String("value2".to_string()));

        let mut override_settings = CustomSettings::new();
        override_settings.set("key2", toml::Value::String("overridden".to_string()));
        override_settings.set("key3", toml::Value::String("value3".to_string()));

        base.merge(override_settings);

        assert_eq!(base.get_str("key1"), Some("value1"));
        assert_eq!(base.get_str("key2"), Some("overridden"));
        assert_eq!(base.get_str("key3"), Some("value3"));
    }

    #[test]
    fn test_custom_settings_from_toml() {
        let toml_str = r#"
            name = "test"
            nested.key = "value"
            nested.deep.value = 42
        "#;

        let value: toml::Value = toml::from_str(toml_str).unwrap();
        let custom = CustomSettings::from_toml(value);

        assert_eq!(custom.get_str("name"), Some("test"));
        assert_eq!(custom.get_str("nested.key"), Some("value"));
        assert_eq!(custom.get_int("nested.deep.value"), Some(42));
    }

    #[test]
    fn test_observability_for_environment() {
        let dev = ObservabilitySettings::for_environment(Environment::Development);
        assert_eq!(dev.log_level, "debug");
        assert_eq!(dev.log_format, "pretty");
        assert!(!dev.tracing_enabled);

        let prod = ObservabilitySettings::for_environment(Environment::Production);
        assert_eq!(prod.log_level, "warn");
        assert_eq!(prod.log_format, "json");
        assert!(prod.tracing_enabled);
    }

    #[test]
    fn test_validation_tls_required_in_production() {
        let mut config = FundamentalConfig::new(Environment::Production);
        config.server.tls = TlsPosture::Disabled;

        let result = config.validate();
        assert!(result.is_err());

        config.server.tls = TlsPosture::Enabled;
    }

    #[test]
    fn terminated_upstream_satisfies_the_deployed_tls_rule() {
        let mut config = FundamentalConfig::new(Environment::Production);
        config.api.tls = TlsPosture::TerminatedUpstream;
        config.server.tls = TlsPosture::TerminatedUpstream;

        assert!(
            config.validate().is_ok(),
            "a proxied deployment must be expressible without claiming to terminate TLS"
        );
    }

    #[test]
    fn disabled_tls_is_still_rejected_when_deployed() {
        let mut config = FundamentalConfig::new(Environment::Production);
        config.api.tls = TlsPosture::Disabled;
        config.server.tls = TlsPosture::TerminatedUpstream;

        let error = config
            .validate()
            .expect_err("plaintext with no ingress is the misconfiguration this catches");

        let crate::ConfigError::MultipleValidationErrors { errors, .. } = &error else {
            panic!("expected a validation-error list, got {error:?}");
        };
        assert!(
            errors.iter().any(|message| message.contains("API TLS")),
            "the failure should name the offending surface: {errors:?}"
        );
        assert!(
            error.to_string().contains("API TLS"),
            "the rendered error must carry the reasons, not just a count: {error}"
        );
    }

    #[test]
    fn test_validation_passes_in_development() {
        let config = FundamentalConfig::new(Environment::Development);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_auth_provider_settings_deserialization() {
        let toml_str = r#"
            enabled = true
            token_key_id = "test-1"
            access_token_ttl = 900
            refresh_token_ttl = 604800

            [providers.kratos]
            enabled = true
            public_url = "http://kratos:4433"
            admin_url = "http://kratos:4434"

            [providers.keto]
            enabled = true
            read_url = "http://keto:4466"
            write_url = "http://keto:4467"
        "#;

        let auth: AuthSettings = toml::from_str(toml_str).unwrap();
        assert!(auth.enabled);

        let kratos = auth.providers.kratos.unwrap();
        assert!(kratos.enabled);
        assert_eq!(kratos.public_url, "http://kratos:4433");
        assert_eq!(kratos.admin_url, "http://kratos:4434");

        let keto = auth.providers.keto.unwrap();
        assert!(keto.enabled);
        assert_eq!(keto.read_url, "http://keto:4466");
        assert_eq!(keto.write_url, "http://keto:4467");

        assert!(auth.providers.hydra.is_none());
        assert!(auth.providers.oathkeeper.is_none());
    }
}
