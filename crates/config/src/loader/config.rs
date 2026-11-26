use super::*;
use crate::settings::CustomSettings;
use crate::source::{EnvVarSource, FundamentalTomlSource, project_manifest};
use crate::source::{SecretBaseDir, expand_env_vars_in_value, validate_override_keys};
use crate::{
    ConfigError, ConfigResult, ConfigSource, EnvironmentOrigin, FundamentalConfig,
    resolve_environment,
};
use fundamental_manifest::Manifest;

use fundamental_core::env::EnvVars;
use fundamental_core::{BuildProfile, Environment};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Loads and merges configuration from multiple sources.
///
/// Later sources override earlier: defaults, `fun.toml`, `fun.{env}.toml`, `FUNDAMENTAL_*`.
///
/// # Examples
///
/// ```rust,no_run
/// use fundamental_config::ConfigLoader;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Auto-detect environment and use default sources
/// let config = ConfigLoader::auto()
///     .with_fundamental_toml("fun.toml")
///     .with_env_vars()
///     .load()?;
///
/// println!("Environment: {}", config.environment);
/// println!("Server port: {}", config.server.port);
/// # Ok(())
/// # }
/// ```
pub struct ConfigLoader {
    origin: EnvironmentOrigin,
    profile: BuildProfile,
    sources: Vec<Box<dyn ConfigSource>>,
    /// Built at `load()` from the same manifest the environment is resolved from.
    manifest: Option<(usize, PathBuf)>,
    table: Option<String>,
    expand_env_vars: bool,
    app: Option<String>,
    instance: Option<String>,
    /// Where `FUNDAMENTAL_*` is read from, so a test need not mutate the process environment.
    vars: EnvVars,
}

impl ConfigLoader {
    /// Creates a config loader for an environment chosen in code; no variable or manifest loosens it.
    pub fn new(environment: Environment) -> Self {
        Self::from_origin(EnvironmentOrigin::Chosen {
            environment,
            name: None,
        })
    }

    fn from_origin(origin: EnvironmentOrigin) -> Self {
        Self {
            origin,
            profile: BuildProfile::CURRENT,
            sources: Vec::new(),
            manifest: None,
            table: None,
            expand_env_vars: true,
            app: None,
            instance: None,
            vars: EnvVars::process(),
        }
    }

    /// Creates a config loader whose environment `FUNDAMENTAL_ENV` names, decided at [`load`](Self::load).
    #[must_use]
    pub fn auto() -> Self {
        Self::auto_from(EnvVars::process())
    }

    /// The same resolution, reading `vars` instead of the process environment.
    #[must_use]
    pub fn auto_from(vars: EnvVars) -> Self {
        Self::from_origin(EnvironmentOrigin::from_vars(&vars)).reading_env_from(vars)
    }

    /// Read `FUNDAMENTAL_*` from `vars` rather than the process environment.
    #[must_use]
    pub fn reading_env_from(mut self, vars: EnvVars) -> Self {
        self.vars = vars;
        self
    }

    /// Creates a config loader for development.
    #[must_use]
    pub fn development() -> Self {
        Self::new(Environment::Development)
    }

    /// Creates a config loader for production.
    #[must_use]
    pub fn production() -> Self {
        Self::new(Environment::Production)
    }

    /// Where this loader's environment comes from; it is decided at [`load`](Self::load).
    #[must_use]
    pub fn origin(&self) -> &EnvironmentOrigin {
        &self.origin
    }

    /// Reads `[environments.{name}]` and `config/{name}.toml` instead of the resolved environment's own.
    #[must_use]
    pub fn with_env_name(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        match &mut self.origin {
            EnvironmentOrigin::Chosen { name: table, .. } => *table = Some(name),
            EnvironmentOrigin::Unset | EnvironmentOrigin::Variable(_) => self.table = Some(name),
        }
        self
    }

    /// Resolves the environment under a release build's rules, whatever this binary was built as.
    #[must_use]
    pub fn as_release_build(mut self) -> Self {
        self.profile = BuildProfile::Release;
        self
    }

    /// Adds `fun.toml` at `path` and the file beside it that overrides it in the load's environment.
    #[must_use]
    pub fn with_fundamental_toml(mut self, path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        self.manifest = Some((self.sources.len(), path));
        self
    }

    /// Adds `FUNDAMENTAL_*` environment variables as a configuration source.
    #[must_use]
    pub fn with_env_vars(mut self) -> Self {
        self.sources.push(Box::new(
            EnvVarSource::fundamental().with_vars(self.vars.clone()),
        ));
        self
    }

    /// Adds a custom configuration source.
    #[must_use]
    pub fn with_source(mut self, source: impl ConfigSource + 'static) -> Self {
        self.sources.push(Box::new(source));
        self
    }

    /// Disables environment variable expansion in config values.
    #[must_use]
    pub fn without_env_expansion(mut self) -> Self {
        self.expand_env_vars = false;
        self
    }

    /// Sets the active app whose sections are promoted to the top level.
    #[must_use]
    pub fn with_app(mut self, app: impl Into<String>) -> Self {
        self.app = Some(app.into());
        self
    }

    /// Loads `[apps.<instance>]`, an `instance_of` this loader's app, instead of `FUNDAMENTAL_INSTANCE`.
    #[must_use]
    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    /// The app section this loader will load: the instance when one is named, else the app.
    #[must_use]
    pub fn resolved_app_name(&self) -> String {
        self.active_instance()
            .unwrap_or_else(|| self.active_app_name())
    }

    fn active_instance(&self) -> Option<String> {
        self.instance
            .clone()
            .or_else(|| self.vars.get(super::INSTANCE_ENV))
            .filter(|instance| !instance.trim().is_empty())
    }

    /// Deprecated alias for [`with_app`](Self::with_app).
    #[deprecated(note = "renamed to with_app")]
    #[must_use]
    pub fn with_surface_group(self, group: impl Into<String>) -> Self {
        self.with_app(group)
    }

    /// Explicit field, then `FUNDAMENTAL_APP`, then deprecated `FUNDAMENTAL_SURFACE_GROUP`, then `"main"`.
    fn active_app_name(&self) -> String {
        if let Some(app) = &self.app {
            return app.clone();
        }
        if let Some(app) = self.vars.get("FUNDAMENTAL_APP") {
            return app;
        }
        if let Some(app) = self.vars.get("FUNDAMENTAL_SURFACE_GROUP") {
            warn!(
                deprecated_env = "FUNDAMENTAL_SURFACE_GROUP",
                "use FUNDAMENTAL_APP instead of FUNDAMENTAL_SURFACE_GROUP"
            );
            return app;
        }
        "main".to_string()
    }

    /// Loads the configuration from all sources.
    ///
    /// # Errors
    /// An unresolvable environment (`CFG_019`, `CFG_020`, `CFG_021`, `CFG_004`), or a source that
    /// fails to load, parse or expand.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub fn load(mut self) -> ConfigResult<FundamentalConfig> {
        // The environment is decided first: it picks the overriding file and whether `.dev.vars` loads.
        let project = match self.manifest.take() {
            Some((at, path)) => project_manifest(&path, true)?.map(|manifest| (at, manifest)),
            None => None,
        };
        let secret_root = project
            .as_ref()
            .map(|(_, manifest)| manifest.root().to_path_buf());
        let declared_table = project
            .as_ref()
            .map(|(_, manifest)| toml::Value::Table(manifest.as_table().clone()));
        let declared = declared_table.as_ref();
        let mut resolved = resolve_environment(self.origin.clone(), declared, self.profile)?;
        if let Some(table) = self.table.take() {
            if resolved.name() != table {
                warn!(
                    environment = resolved.name(),
                    table = %table,
                    "reading a table other than the environment's own; the environment still decides the rules"
                );
            }
            resolved = resolve_environment(
                EnvironmentOrigin::Chosen {
                    environment: resolved.environment(),
                    name: Some(table),
                },
                declared,
                self.profile,
            )?;
        }
        let environment = resolved.environment();
        let env_name = resolved.custom_name().map(str::to_owned);
        let app_name = self.active_app_name();
        info!(environment = resolved.name(), app = %app_name, "loading configuration");

        if let Some((at, manifest)) = project {
            let settings = Manifest::load_for(manifest.root(), resolved.name())?;
            self.sources.insert(
                at,
                Box::new(FundamentalTomlSource {
                    manifest: toml::Value::Table(settings.as_table().clone()),
                }),
            );
        }

        // Development only, non-overriding.
        if environment.is_development() {
            crate::source::load_dev_vars();
        }

        let mut merged = toml::Value::Table(toml::map::Map::new());
        let binary = app_name;
        let instance = self
            .active_instance()
            .filter(|instance| *instance != binary);
        // An instance's listener host and TLS variables are laid over its section once it is resolved.
        let mut runtime_instance = None;

        for source in &self.sources {
            debug!(source = source.name(), "loading from config source");
            if let Some(mut value) = source.load()? {
                if source.name() == "env-vars"
                    && let Some(instance) = instance.as_deref()
                {
                    runtime_instance = value
                        .get_mut("apps")
                        .and_then(|apps| apps.get_mut(instance))
                        .and_then(take_listener_addresses);
                }
                // Env var sources use a different key mapping.
                if source.name() != "fun.toml" && source.name() != "env-vars" {
                    validate_override_keys(&merged, &value, "");
                }
                merge_toml(&mut merged, value);
            }
        }

        // Promote before expansion, so `${VAR}` in app-scoped sections is expanded at the top level.
        let app_name = super::resolve_instance(&mut merged, &binary, instance.as_deref())?;
        if app_name != binary {
            info!(app = %binary, instance = %app_name, "loading an app instance");
        }
        if let Some(runtime) = runtime_instance {
            debug!(instance = %app_name, "laying the instance's variables over its effective section");
            if let Some(effective) = merged
                .get_mut("apps")
                .and_then(|apps| apps.get_mut(app_name.as_str()))
            {
                merge_toml(effective, runtime);
            }
        }
        promote_app_config(&mut merged, &app_name);

        // A relative `${file:…}` is anchored to the project root, whatever the working directory.
        if self.expand_env_vars {
            let base = secret_root.map(SecretBaseDir::new).unwrap_or_default();
            expand_env_vars_in_value(&mut merged, "", &base)?;
        }

        let mut config = Self::toml_to_config(environment, env_name, merged)?;

        merge_surface_entry_config(&mut config, &app_name);

        info!(
            environment = %config.environment_name(),
            app = %app_name,
            api.host = %config.api.host,
            api.port = config.api.port,
            ssr.host = %config.server.host,
            ssr.port = config.server.port,
            "configuration loaded"
        );

        Ok(config)
    }

    /// Parses the merged value, whose app sections are already promoted to the top level.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "readability baseline"
    )]
    fn toml_to_config(
        environment: Environment,
        env_name: Option<String>,
        value: toml::Value,
    ) -> ConfigResult<FundamentalConfig> {
        let table = match value {
            toml::Value::Table(t) => t,
            _ => return Err(ConfigError::validation("root config must be a table")),
        };

        let mut config = FundamentalConfig::new(environment);
        config.env_name = env_name;

        if let Some(project) = table.get("project") {
            config.project = project.clone().try_into().map_err(|e: toml::de::Error| {
                ConfigError::invalid_value("project", e.to_string())
            })?;
        }

        if let Some(domains) = table.get("domains") {
            config.domains = parse_domains_config(domains)?;
        }

        if let Some(database) = table.get("database") {
            config.database = parse_database_settings(database)?;
        }
        if let Some(databases) = table.get("databases") {
            warn!("[databases] is deprecated in fun.toml — use [database] (singular) instead");
            config.database = parse_database_settings(databases)?;
        }

        if let Some(encryption) = table.get("encryption") {
            config.encryption = encryption
                .clone()
                .try_into()
                .map_err(|e: toml::de::Error| {
                    ConfigError::invalid_value("encryption", e.to_string())
                })?;
            config.encryption.require_declared_vault_keys(environment)?;
        }

        if let Some(vault) = table.get("vault") {
            config.secrets = vault
                .clone()
                .try_into()
                .map_err(|e: toml::de::Error| ConfigError::invalid_value("vault", e.to_string()))?;
        }

        if let Some(messaging) = table.get("messaging") {
            config.messaging = parse_messaging_settings(messaging)?;
            if let Some(nats) = &config.messaging.nats {
                nats.require_deployed_credentials(environment)?;
            }
        }

        if let Some(i18n) = table.get("i18n") {
            config.translations = i18n
                .clone()
                .try_into()
                .map_err(|e: toml::de::Error| ConfigError::invalid_value("i18n", e.to_string()))?;
        }

        // `groups` is the deprecated key, already warned about by the caller.
        if let Some(apps) = table.get("apps") {
            config.apps = parse_apps_config(apps)?;
        } else if let Some(groups) = table.get("groups") {
            warn!(
                deprecated_key = "groups",
                "use [apps.*] instead of [groups.*]"
            );
            config.apps = parse_apps_config(groups)?;
        }

        if let Some(auth) = table.get("auth") {
            config.auth = auth
                .clone()
                .try_into()
                .map_err(|e: toml::de::Error| ConfigError::invalid_value("auth", e.to_string()))?;
            config.auth.enabled = true;
        }

        if let Some(authz) = table.get("authz") {
            config.authz = authz
                .clone()
                .try_into()
                .map_err(|e: toml::de::Error| ConfigError::invalid_value("authz", e.to_string()))?;
            config.authz.enabled = true;
        }

        if let Some(client) = table.get("client") {
            config.client = client.clone().try_into().map_err(|e: toml::de::Error| {
                ConfigError::invalid_value("client", e.to_string())
            })?;
        }

        // Over the environment-derived defaults: on its own the table would reset unset keys to development's.
        if let Some(observability) = table.get("observability") {
            let mut merged = toml::Value::try_from(&config.observability).map_err(|e| {
                ConfigError::invalid_value("observability", format!("defaults: {e}"))
            })?;
            crate::loader::merge::merge_toml(&mut merged, observability.clone());
            config.observability = merged.try_into().map_err(|e: toml::de::Error| {
                ConfigError::invalid_value("observability", e.to_string())
            })?;
        }

        // Raw TOML, deserialized by fundamental_manager.
        if let Some(cms) = table.get("cms") {
            config.cms = Some(cms.clone());
        }

        if let Some(manager) = table.get("manager") {
            config.manager = Some(manager.clone());
        }

        if let Some(custom) = table.get("custom") {
            config.custom = CustomSettings::from_toml(custom.clone());
        }

        Ok(config)
    }
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::auto()
    }
}

#[cfg(test)]
mod i18n_section_tests {
    use super::*;

    #[test]
    fn loads_i18n_section_from_toml() {
        let toml = r#"
            [i18n]
            enabled = true
            source_locale = "en"
            default_locale = "es"
            supported_locales = ["en", "es", "fr"]
            fallback_chain = ["es"]
        "#;
        let value: toml::Value = toml::from_str(toml).unwrap();
        let config = ConfigLoader::toml_to_config(Environment::Development, None, value).unwrap();

        assert!(config.translations.enabled);
        assert_eq!(config.translations.default_locale(), "es");
        assert_eq!(config.translations.supported_locales, ["en", "es", "fr"]);
        assert_eq!(config.translations.fallback_chain, ["es"]);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn invalid_i18n_locales_fail_validation() {
        let toml = r#"
            [i18n]
            enabled = true
            default_locale = "de"
            supported_locales = ["en"]
        "#;
        let value: toml::Value = toml::from_str(toml).unwrap();
        let config = ConfigLoader::toml_to_config(Environment::Development, None, value).unwrap();
        assert!(config.validate().is_err());
    }
}

impl std::fmt::Debug for ConfigLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigLoader")
            .field("origin", &self.origin)
            .field("profile", &self.profile)
            .field("sources", &self.sources.len())
            .field("expand_env_vars", &self.expand_env_vars)
            .finish()
    }
}

#[cfg(test)]
mod auto_env_tests {
    use super::*;

    use fundamental_core::{EnvironmentPosture, Error as _};
    use tempfile::TempDir;
    use tracing_test::traced_test;

    #[test]
    fn auto_records_the_origin_and_leaves_the_decision_to_load() {
        let var = Environment::ENV_VAR;
        for (vars, origin) in [
            (
                EnvVars::fixed([(var, "production")]),
                EnvironmentOrigin::Variable("production".to_owned()),
            ),
            (
                EnvVars::fixed([(var, "devops")]),
                EnvironmentOrigin::Variable("devops".to_owned()),
            ),
            (EnvVars::fixed([(var, "  ")]), EnvironmentOrigin::Unset),
            (EnvVars::none(), EnvironmentOrigin::Unset),
        ] {
            assert_eq!(ConfigLoader::auto_from(vars).origin(), &origin);
        }
    }

    #[test]
    fn the_environment_auto_read_from_is_the_one_the_sources_read() {
        let loader = ConfigLoader::auto_from(EnvVars::fixed([
            (Environment::ENV_VAR, "production"),
            ("FUNDAMENTAL_APP", "billing"),
        ]));
        assert_eq!(loader.active_app_name(), "billing");
    }

    fn manifest(dir: &TempDir, body: &str) -> PathBuf {
        let path = dir.path().join("fun.toml");
        std::fs::write(&path, body).expect("write manifest");
        path
    }

    #[test]
    fn a_release_load_refuses_an_unset_environment() {
        let dir = TempDir::new().expect("tempdir");
        let path = manifest(&dir, "[project]\nname = \"p\"\n");
        let err = ConfigLoader::auto_from(EnvVars::none())
            .as_release_build()
            .with_fundamental_toml(&path)
            .load()
            .expect_err("a release build never assumes development");
        assert_eq!(err.error_code(), "CFG_019");
    }

    #[test]
    fn a_declared_deployed_posture_enforces_every_deployed_rule() {
        let dir = TempDir::new().expect("tempdir");
        let path = manifest(
            &dir,
            r#"
            [environments.devops]
            posture = "deployed"

            [apps.main.messaging.nats]
            url = "nats://nats.internal:4222"
            user = "svc"
            password = ""
            "#,
        );
        let load = || {
            ConfigLoader::auto_from(EnvVars::fixed([(Environment::ENV_VAR, "devops")]))
                .as_release_build()
                .with_fundamental_toml(&path)
                .load()
        };
        let err = load().expect_err("a NATS secret is refused when deployed");
        assert!(
            matches!(&err, ConfigError::InvalidValue { field, .. } if field == "messaging.nats"),
            "{err:?}"
        );

        std::fs::write(
            &path,
            "[environments.devops]\nposture = \"deployed\"\n[apps.main]\nweb = { port = 1 }\n",
        )
        .expect("rewrite manifest");
        let config = load().expect("declared deployed loads");
        assert_eq!(config.environment, Environment::Production);
        assert_eq!(config.environment.posture(), EnvironmentPosture::Deployed);
        assert_eq!(config.environment_name(), "devops");
        assert!(
            config.validate().is_err(),
            "deployed with no secret store and no TLS must not validate"
        );
    }

    #[traced_test]
    #[test]
    fn a_debug_load_of_an_undeclared_name_runs_as_development_and_warns() {
        let dir = TempDir::new().expect("tempdir");
        let path = manifest(
            &dir,
            "[environments.devops]\n[apps.main]\nweb = { port = 1 }\n",
        );
        let config = ConfigLoader::auto_from(EnvVars::fixed([(Environment::ENV_VAR, "devops")]))
            .with_fundamental_toml(&path)
            .load()
            .expect("a debug build runs it");
        assert_eq!(config.environment, Environment::Development);
        assert_eq!(config.environment_name(), "devops");
        assert!(logs_contain("declares no posture"));
    }

    #[traced_test]
    #[test]
    fn a_table_named_in_code_never_loosens_the_variable() {
        let dir = TempDir::new().expect("tempdir");
        let path = manifest(
            &dir,
            "[environments.laptop]\nposture = \"development\"\n[environments.edge]\n",
        );
        let production = || EnvVars::fixed([(Environment::ENV_VAR, "production")]);

        let err = ConfigLoader::auto_from(production())
            .as_release_build()
            .with_env_name("laptop")
            .with_fundamental_toml(&path)
            .load()
            .expect_err("a development table cannot loosen production");
        assert_eq!(err.error_code(), "CFG_021");
        assert!(logs_contain(
            "reading a table other than the environment's own"
        ));

        let config = ConfigLoader::auto_from(production())
            .as_release_build()
            .with_env_name("edge")
            .with_fundamental_toml(&path)
            .load()
            .expect("a table without a posture is read under production's rules");
        assert_eq!(config.environment, Environment::Production);
        assert_eq!(config.environment_name(), "edge");

        let err = ConfigLoader::auto_from(EnvVars::none())
            .as_release_build()
            .with_env_name("edge")
            .with_fundamental_toml(&path)
            .load()
            .expect_err("naming a table does not name an environment");
        assert_eq!(err.error_code(), "CFG_019");
    }

    #[test]
    fn a_name_chosen_after_the_manifest_is_the_name_it_is_read_under() {
        let dir = TempDir::new().expect("tempdir");
        let path = manifest(&dir, "[environments.devops]\nposture = \"development\"\n");
        let err = ConfigLoader::new(Environment::Production)
            .with_fundamental_toml(&path)
            .with_env_name("devops")
            .load()
            .expect_err("production cannot be loosened by the table it reads");
        assert_eq!(err.error_code(), "CFG_021");
    }
}
