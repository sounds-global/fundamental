//! The root configuration type ([`FundamentalConfig`]).

use super::*;
use crate::{ConfigError, ConfigResult};
use fundamental_core::Environment;

const API_SURFACE: &str = "api";

/// The canonical `web` name plus the legacy spellings the loader still merges from.
const WEB_SURFACES: &[&str] = &["web", "ssr", "web-ssr"];

/// A client-facing listener promoted into neither, so read from its own entry.
const SYNC_SURFACE: &str = "sync";

/// The unified Fundamental configuration, populated by the [`ConfigLoader`](crate::ConfigLoader).
///
/// # Examples
///
/// ```rust,no_run
/// use fundamental_config::ConfigLoader;
///
/// # fn main() -> fundamental_config::ConfigResult<()> {
/// let config = ConfigLoader::auto()
///     .with_fundamental_toml("fun.toml")
///     .with_env_vars()
///     .load()?;
///
/// println!("Running in {} environment", config.environment);
/// println!("API port: {}", config.api.port);
/// println!("SSR port: {}", config.server.port);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct FundamentalConfig {
    /// The deployment environment.
    pub environment: Environment,
    /// Custom environment name (e.g. "devops"); `None` for standard environments.
    pub env_name: Option<String>,
    /// Project metadata (name, version, type).
    pub project: ProjectConfig,
    /// API server configuration (fundamental_api).
    pub api: ApiSettings,
    /// SSR server configuration (fundamental_web_ssr).
    pub server: ServerSettings,
    /// Database configuration.
    pub database: DatabaseSettings,
    /// Field-level encryption configuration.
    pub encryption: EncryptionSettings,
    /// Secret-manager (HashiCorp Vault) configuration, shared by database
    /// credential resolution and encryption key material.
    pub secrets: VaultSettings,
    /// Messaging configuration.
    pub messaging: MessagingSettings,
    /// App configurations (for `type = "service"` projects).
    pub apps: AppsConfig,
    /// Domain configurations (for `type = "domain"` projects).
    pub domains: DomainsConfig,
    /// Authentication configuration.
    pub auth: AuthSettings,
    /// Authorization engine configuration (`fundamental_authz`).
    pub authz: AuthzSettings,
    /// Client configuration.
    pub client: ClientSettings,
    /// Observability configuration.
    pub observability: ObservabilitySettings,
    /// Localization configuration (catalogs, locales, loading strategy).
    pub translations: TranslationSettings,
    /// CMS configuration, raw so the manager crate deserializes it without a dependency cycle.
    pub cms: Option<toml::Value>,
    /// Content manager configuration, raw so `fundamental_manager` deserializes it.
    pub manager: Option<toml::Value>,
    /// Custom application-specific settings.
    pub custom: CustomSettings,
}

impl FundamentalConfig {
    /// Creates a new configuration with defaults for the given environment.
    #[must_use]
    pub fn new(environment: Environment) -> Self {
        Self {
            environment,
            env_name: None,
            project: ProjectConfig::default(),
            api: ApiSettings::default(),
            server: ServerSettings::default(),
            database: DatabaseSettings::default(),
            encryption: EncryptionSettings::default(),
            secrets: VaultSettings::default(),
            messaging: MessagingSettings::default(),
            apps: AppsConfig::default(),
            domains: DomainsConfig::default(),
            auth: AuthSettings::default(),
            authz: AuthzSettings::default(),
            client: ClientSettings::default(),
            observability: ObservabilitySettings::for_environment(environment),
            translations: TranslationSettings::default(),
            cms: None,
            manager: None,
            custom: CustomSettings::default(),
        }
    }

    /// Returns `true` if this is a domain-type project (owns models + migrations, no services).
    #[must_use]
    pub fn is_domain_project(&self) -> bool {
        self.project.project_type == ProjectType::Domain
    }

    /// The custom environment name if set, else the [`Environment`]'s canonical name.
    #[must_use]
    pub fn environment_name(&self) -> &str {
        self.env_name
            .as_deref()
            .unwrap_or(self.environment.as_str())
    }

    /// Validates the configuration for the current environment.
    ///
    /// # Errors
    /// A validation error, such as TLS not configured or required secrets missing when deployed.
    pub fn validate(&self) -> ConfigResult<()> {
        self.validate_surfaces(None)
    }

    /// Validates as `app_name` runs it: a listener's TLS is required only when it declares that surface.
    ///
    /// # Errors
    /// As [`validate`](Self::validate).
    pub fn validate_for_app(&self, app_name: &str) -> ConfigResult<()> {
        self.validate_surfaces(Some(app_name))
    }

    /// The named surface entry `app_name` declares, when enabled; `None` with no active app.
    fn enabled_surface(
        &self,
        app_name: Option<&str>,
        surface: &str,
    ) -> Option<&SurfaceEntryConfig> {
        self.apps
            .apps
            .get(app_name?)?
            .surfaces
            .get(surface)
            .filter(|entry| entry.enabled)
    }

    /// With no active app every listener is in scope, since the binding subset is unknown.
    fn app_runs_surface(&self, app_name: Option<&str>, surfaces: &[&str]) -> bool {
        let Some(app_name) = app_name else {
            return true;
        };
        let Some(app) = self.apps.apps.get(app_name) else {
            return false;
        };
        surfaces.iter().any(|surface| {
            app.surfaces
                .get(*surface)
                .is_some_and(|entry| entry.enabled)
        })
    }

    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn validate_surfaces(&self, app_name: Option<&str>) -> ConfigResult<()> {
        if self.environment.is_deployed() && !self.secrets.enabled {
            let needs_secret_manager = self.encryption.backend == KeyBackend::Vault
                || self.database.requires_secret_manager()
                || self
                    .messaging
                    .nats
                    .as_ref()
                    .is_some_and(NatsSettings::requires_secret_manager);
            if needs_secret_manager {
                return Err(ConfigError::secret_manager_required(
                    self.environment_name(),
                ));
            }
        }

        let mut errors = Vec::new();

        errors.extend(self.translations.validation_errors());

        // Whether traffic reaching a client is encrypted, not whether this socket does the handshake.
        if self.environment.is_deployed() {
            if self.app_runs_surface(app_name, &[API_SURFACE])
                && !self.api.tls.is_encrypted_in_transit()
            {
                errors.push(
                    "API TLS must be enabled in deployed environments — set \
                     `tls = \"enabled\"` to terminate here, or \
                     `tls = \"terminated_upstream\"` if an ingress terminates it"
                        .to_string(),
                );
            }

            if self.app_runs_surface(app_name, WEB_SURFACES)
                && !self.server.tls.is_encrypted_in_transit()
            {
                errors.push(
                    "Server TLS must be enabled in deployed environments — set \
                     `tls = \"enabled\"` to terminate here, or \
                     `tls = \"terminated_upstream\"` if an ingress terminates it"
                        .to_string(),
                );
            }

            // `sync` feeds neither `ApiSettings` nor `ServerSettings`, so read its posture off the entry.
            if let Some(entry) = self.enabled_surface(app_name, SYNC_SURFACE)
                && !entry.tls.unwrap_or_default().is_encrypted_in_transit()
            {
                errors.push(
                    "Sync TLS must be enabled in deployed environments — set \
                     `tls = \"enabled\"` on the `sync` surface to terminate here, or \
                     `tls = \"terminated_upstream\"` if an ingress terminates it"
                        .to_string(),
                );
            }

            // A static token can be neither renewed nor replaced; AppRole and Kubernetes auth log in again.
            if self.secrets.enabled
                && matches!(self.secrets.auth, crate::settings::VaultAuth::Token { .. })
            {
                errors.push(
                    "Vault token auth cannot renew or replace itself — use `method = \"app_role\"` \
                     or `method = \"kubernetes\"` in deployed environments"
                        .to_string(),
                );
            }

            // Every secret crosses this hop, so a deployed client reaches it encrypted or on this host.
            if self.secrets.enabled {
                if self.secrets.tls_skip_verify {
                    errors.push(
                        "the secret store's TLS certificate must be verified in deployed \
                         environments — remove `tls_skip_verify`, and set `ca_cert_path` if a \
                         private CA signs it"
                            .to_string(),
                    );
                }
                if !self.secrets.address.trim().starts_with("https://")
                    && !names_loopback(&self.secrets.address)
                {
                    errors.push(format!(
                        "the secret store is reached over plaintext at `{}` — deployed \
                         environments reach it over `https://`, or on this host",
                        self.secrets.address
                    ));
                }
            }

            // Every connection, named ones included.
            for connection in self.database.connections() {
                if !connection.tls.is_encrypted_in_transit() {
                    errors.push(format!(
                        "database TLS must be enabled in deployed environments for connection \
                         `{}` — set `tls = \"enabled\"`, or `tls = \"terminated_upstream\"` if a \
                         sidecar or mesh proxy carries the encrypted hop",
                        connection.label()
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::MultipleValidationErrors {
                count: errors.len(),
                errors,
            })
        }
    }

    /// Returns the API URL derived from the api settings.
    #[must_use]
    pub fn api_url(&self) -> String {
        self.api.url()
    }
}

impl Default for FundamentalConfig {
    fn default() -> Self {
        Self::new(Environment::default())
    }
}

#[cfg(test)]
mod secret_manager_tests {
    use super::*;
    use crate::settings::{KeyBackend, TlsPosture};

    #[test]
    fn cfg008_when_vault_key_backend_without_secret_manager() {
        let mut config = FundamentalConfig::new(Environment::Production);
        config.api.tls = TlsPosture::Enabled;
        config.server.tls = TlsPosture::Enabled;
        config.encryption.backend = KeyBackend::Vault;
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::SecretManagerRequired { .. }),
            "expected SecretManagerRequired, got {err:?}"
        );
    }

    #[test]
    fn ok_when_vault_backend_and_secret_manager_enabled() {
        let mut config = FundamentalConfig::new(Environment::Production);
        config.api.tls = TlsPosture::Enabled;
        config.server.tls = TlsPosture::Enabled;
        config.encryption.backend = KeyBackend::Vault;
        config.secrets.enabled = true;
        config.secrets.auth = crate::settings::VaultAuth::AppRole {
            role_id: "service".to_owned(),
            secret_id: fundamental_core::secrets::InMemorySecret::new("id".to_owned()),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn token_auth_is_refused_in_production() {
        let mut config = FundamentalConfig::new(Environment::Production);
        config.api.tls = TlsPosture::Enabled;
        config.server.tls = TlsPosture::Enabled;
        config.secrets.enabled = true;
        let error = config.validate().expect_err("static token refused");
        match error {
            ConfigError::MultipleValidationErrors { errors, .. } => {
                assert!(
                    errors.iter().any(|e| e.contains("token auth")),
                    "{errors:?}"
                );
            }
            other => panic!("expected MultipleValidationErrors, got {other:?}"),
        }
    }

    #[test]
    fn token_auth_is_allowed_in_development() {
        let mut config = FundamentalConfig::new(Environment::Development);
        config.secrets.enabled = true;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn development_is_exempt() {
        let mut config = FundamentalConfig::new(Environment::Development);
        config.encryption.backend = KeyBackend::Vault;
        assert!(config.validate().is_ok());
    }
    fn deployed_with_a_secret_store(environment: Environment, address: &str) -> FundamentalConfig {
        let mut config = FundamentalConfig::new(environment);
        config.api.tls = TlsPosture::Enabled;
        config.server.tls = TlsPosture::Enabled;
        config.secrets.enabled = true;
        config.secrets.address = address.to_owned();
        config.secrets.auth = crate::settings::VaultAuth::AppRole {
            role_id: "service".to_owned(),
            secret_id: fundamental_core::secrets::InMemorySecret::new("id".to_owned()),
        };
        config
    }

    fn validation_errors(config: &FundamentalConfig) -> Vec<String> {
        match config.validate() {
            Ok(()) => Vec::new(),
            Err(ConfigError::MultipleValidationErrors { errors, .. }) => errors,
            Err(other) => panic!("expected validation errors, got {other:?}"),
        }
    }

    #[test]
    fn a_nats_credential_in_the_store_needs_the_store_enabled_when_deployed() {
        for credential in [
            "username = \"orders\"\npassword = { source = \"vault_kv\", mount = \"secret\", path = \"nats/orders\", key = \"password\" }\n",
            "nkey = { public_key = \"UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP\", signer = { source = \"vault_transit\", mount = \"transit\", key = \"nats-orders\" } }\n",
        ] {
            let nats: crate::settings::NatsSettings =
                toml::from_str(&format!("url = \"tls://bus:4222\"\n{credential}"))
                    .expect("a credential in the store");
            let mut config = FundamentalConfig::new(Environment::Staging);
            config.api.tls = TlsPosture::Enabled;
            config.server.tls = TlsPosture::Enabled;
            config.messaging.nats = Some(nats);
            let error = config
                .validate()
                .expect_err("nothing could read the reference or sign with the key");
            assert!(
                matches!(error, ConfigError::SecretManagerRequired { .. }),
                "{credential}: expected SecretManagerRequired, got {error:?}"
            );
        }
    }

    #[test]
    fn a_deployed_secret_store_is_reached_encrypted_and_verified_or_on_this_host() {
        for environment in [Environment::Staging, Environment::Production] {
            let plaintext =
                deployed_with_a_secret_store(environment, "http://openbao.internal:8200");
            assert!(
                validation_errors(&plaintext)
                    .iter()
                    .any(|error| error.contains("reached over plaintext")),
                "{environment}"
            );

            let mut unverified =
                deployed_with_a_secret_store(environment, "https://openbao.internal:8200");
            unverified.secrets.tls_skip_verify = true;
            assert!(
                validation_errors(&unverified)
                    .iter()
                    .any(|error| error.contains("must be verified")),
                "{environment}"
            );

            for address in ["https://openbao.internal:8200", "http://127.0.0.1:8200"] {
                let accepted = deployed_with_a_secret_store(environment, address);
                assert_eq!(
                    validation_errors(&accepted),
                    Vec::<String>::new(),
                    "{address}"
                );
            }
        }
        let local = deployed_with_a_secret_store(Environment::Development, "http://openbao:8200");
        assert!(
            local.validate().is_ok(),
            "a local run may use a plaintext store"
        );
    }
}

#[cfg(test)]
mod listener_and_connection_tests {
    use super::*;
    use crate::settings::{
        CqlSettings, DatabaseSettings, NamedDatabaseSettings, SqlSettings, TlsPosture,
    };
    use std::collections::HashMap;

    fn deployed() -> FundamentalConfig {
        let mut config = FundamentalConfig::new(Environment::Production);
        config.api.tls = TlsPosture::TerminatedUpstream;
        config.server.tls = TlsPosture::TerminatedUpstream;
        config
    }

    fn reasons(error: &ConfigError) -> Vec<String> {
        match error {
            ConfigError::MultipleValidationErrors { errors, .. } => errors.clone(),
            other => panic!("expected MultipleValidationErrors, got {other:?}"),
        }
    }

    fn with_named_sql(config: &mut FundamentalConfig, name: &str, tls: TlsPosture) {
        let mut sql = HashMap::new();
        sql.insert(
            name.to_string(),
            SqlSettings {
                tls,
                ..Default::default()
            },
        );
        config.database = DatabaseSettings {
            sql: None,
            cql: None,
            named: Some(NamedDatabaseSettings {
                sql,
                cql: HashMap::new(),
            }),
        };
    }

    #[test]
    fn a_named_connection_without_tls_fails_a_deployed_load() {
        let mut config = deployed();
        with_named_sql(&mut config, "layer0", TlsPosture::Disabled);

        let error = config.validate().expect_err("a plaintext pool is rejected");
        let reasons = reasons(&error);

        assert_eq!(
            reasons.len(),
            1,
            "one reason, naming the offender: {reasons:?}"
        );
        assert!(
            reasons[0].contains("sql.layer0"),
            "the reason must name the connection, got {reasons:?}"
        );
    }

    #[test]
    fn a_named_connection_terminated_upstream_passes() {
        let mut config = deployed();
        with_named_sql(&mut config, "layer0", TlsPosture::TerminatedUpstream);

        assert!(config.validate().is_ok());
    }

    #[test]
    fn every_offending_connection_is_reported_not_just_the_first() {
        let mut config = deployed();
        with_named_sql(&mut config, "layer0", TlsPosture::Disabled);
        config.database.cql = Some(CqlSettings::default());

        let reasons = reasons(&config.validate().expect_err("both pools are plaintext"));

        assert_eq!(
            reasons.len(),
            2,
            "an operator in a crash loop has only logs"
        );
    }

    #[test]
    fn a_sync_listener_without_a_posture_fails_a_deployed_load() {
        let mut config = deployed();
        let mut app = AppConfig::default();
        app.surfaces
            .insert("sync".to_string(), SurfaceEntryConfig::default());
        config.apps.apps.insert("holder_vault".to_string(), app);

        let reasons = reasons(
            &config
                .validate_for_app("holder_vault")
                .expect_err("a sync listener with no stated posture is rejected"),
        );

        assert!(
            reasons.iter().any(|r| r.contains("Sync TLS")),
            "got {reasons:?}"
        );
    }

    #[test]
    fn a_sync_listener_terminated_upstream_passes() {
        let mut config = deployed();
        let mut app = AppConfig::default();
        let entry = SurfaceEntryConfig {
            tls: Some(TlsPosture::TerminatedUpstream),
            ..Default::default()
        };
        app.surfaces.insert("sync".to_string(), entry);
        config.apps.apps.insert("holder_vault".to_string(), app);

        assert!(config.validate_for_app("holder_vault").is_ok());
    }

    #[test]
    fn an_admin_listener_needs_no_posture() {
        let mut config = deployed();
        let mut app = AppConfig::default();
        app.surfaces
            .insert("admin".to_string(), SurfaceEntryConfig::default());
        config.apps.apps.insert("layer5_monitor".to_string(), app);

        assert!(config.validate_for_app("layer5_monitor").is_ok());
    }
}
