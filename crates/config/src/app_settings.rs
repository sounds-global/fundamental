//! [`AppSettings`]: a loaded configuration handle for the active app.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use fundamental_core::Environment;
use fundamental_core::listen::{Listen, SocketName};
use tracing::{info, instrument};

use crate::settings::{ListenMode, NatsSettings, SurfaceEntryConfig};
use crate::{ConfigError, ConfigLoader, ConfigResult, FundamentalConfig};

const SOCKET_SURFACE: &str = "socket";

const API_SURFACE: &str = "api";

const WEB_SURFACE: &str = "web";

const SYNC_SURFACE: &str = "sync";

const ADMIN_SURFACE: &str = "admin";

const DEFAULT_SURFACE_HOST: &str = "127.0.0.1";

/// The comma-separated surface allowlist `fun dev <app> <surface>...` sets.
const SURFACES_ENV: &str = "FUNDAMENTAL_SURFACES";

/// A loaded configuration paired with the active app it was loaded for.
#[derive(Debug, Clone)]
pub struct AppSettings {
    name: String,
    /// The app the code named, which [`name`](Self::name) is an instance of when they differ.
    base: String,
    config: FundamentalConfig,
    /// `None` means every surface runs.
    surfaces_filter: Option<Vec<String>>,
}

impl AppSettings {
    /// The manifest [`AppSettings::load`] reads, relative to the working directory.
    pub const MANIFEST: &'static str = "fun.toml";

    /// Loads the configuration for app `name` from `fun.toml`.
    ///
    /// # Errors
    /// A [`ConfigError`] if a source cannot be read or parsed, or validation fails.
    #[instrument(err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn load(name: &str) -> ConfigResult<Self> {
        Self::load_from(name, Self::MANIFEST)
    }

    /// Loads the configuration for app `name` from the manifest at `toml_path`.
    ///
    /// # Errors
    /// A [`ConfigError`] if a source cannot be read or parsed, or validation fails.
    #[instrument(skip(toml_path), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn load_from(name: &str, toml_path: impl AsRef<Path>) -> ConfigResult<Self> {
        Self::load_resolved(name, toml_path, ConfigLoader::auto())?.claiming_passed_sockets()
    }

    /// Loads app `name` in an explicitly supplied environment, ignoring `FUNDAMENTAL_ENV`.
    ///
    /// # Errors
    /// As [`AppSettings::load_from`].
    #[instrument(skip(toml_path), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn load_in(
        name: &str,
        toml_path: impl AsRef<Path>,
        environment: fundamental_core::Environment,
    ) -> ConfigResult<Self> {
        Self::load_resolved(name, toml_path, ConfigLoader::new(environment))?
            .claiming_passed_sockets()
    }

    /// The same load, reading `FUNDAMENTAL_*` from `vars` rather than the process.
    ///
    /// # Errors
    /// The same as [`load_in`](Self::load_in).
    pub fn load_in_reading_env_from(
        name: &str,
        toml_path: impl AsRef<Path>,
        environment: fundamental_core::Environment,
        vars: fundamental_core::env::EnvVars,
    ) -> ConfigResult<Self> {
        Self::load_resolved(
            name,
            toml_path,
            ConfigLoader::new(environment).reading_env_from(vars),
        )?
        .claiming_passed_sockets()
    }

    /// Loads `[apps.<instance>]`, a deployment of `base`'s code, whatever instance this process runs as.
    ///
    /// # Errors
    /// As [`AppSettings::load_from`]; `CFG_015` when `instance` is undeclared, `CFG_016` when it
    /// is an instance of another app.
    #[instrument(err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn load_instance(base: &str, instance: &str) -> ConfigResult<Self> {
        Self::load_instance_from(base, instance, Self::MANIFEST)
    }

    /// [`AppSettings::load_instance`], reading the manifest at `toml_path`.
    ///
    /// # Errors
    /// As [`AppSettings::load_instance`].
    #[instrument(skip(toml_path), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn load_instance_from(
        base: &str,
        instance: &str,
        toml_path: impl AsRef<Path>,
    ) -> ConfigResult<Self> {
        Self::load_resolved(
            base,
            toml_path,
            ConfigLoader::auto().with_instance(instance),
        )
    }

    /// Loads app `name` from a release's manifest, in a deployed environment, claiming no socket.
    ///
    /// # Errors
    /// As [`AppSettings::load_from`]; [`ConfigError::ReleaseNotDeployed`] (`CFG_030`) in
    /// development or test.
    #[instrument(skip(manifest), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn load_release(name: &str, manifest: impl AsRef<Path>) -> ConfigResult<Self> {
        Self::load_release_with(name, manifest, ConfigLoader::auto())
    }

    fn load_release_with(
        name: &str,
        manifest: impl AsRef<Path>,
        loader: ConfigLoader,
    ) -> ConfigResult<Self> {
        let app = Self::load_resolved(name, manifest, loader)?;
        if !app.environment().is_deployed() {
            return Err(ConfigError::ReleaseNotDeployed {
                environment: app.environment().to_string(),
            });
        }
        Ok(app)
    }

    fn load_resolved(
        name: &str,
        toml_path: impl AsRef<Path>,
        loader: ConfigLoader,
    ) -> ConfigResult<Self> {
        let toml_path = toml_path.as_ref();
        let declared = declared_key(name, toml_path);
        let name = declared.as_str();
        // Read beside the file `fun.toml` really is, so settings come from one release however a link moves.
        let loader = loader
            .with_app(name)
            .with_fundamental_toml(toml_path)
            .with_env_vars();
        // The section this process *is*: an instance of `name` when the deployment names one.
        let instance = loader.resolved_app_name();
        let config = loader.load()?;

        // Fail the boot, not the first request.
        config.validate_for_app(&instance)?;

        info!(app = %instance, base = name, env = %config.environment, "app config loaded");
        Ok(Self {
            name: instance,
            base: name.to_string(),
            config,
            surfaces_filter: parse_surfaces_filter(std::env::var(SURFACES_ENV).ok().as_deref()),
        })
    }

    /// Refuses the boot when the passed sockets and the surfaces listening on them disagree.
    #[cfg(unix)]
    fn claiming_passed_sockets(self) -> ConfigResult<Self> {
        let activated = self
            .surfaces()
            .filter(|(_, entry)| entry.listen == ListenMode::Activated)
            .map(|(surface, _)| surface.as_str());
        crate::activation_check::activation_check(
            activated,
            fundamental_core::listen::ActivatedSockets::inherited().names(),
        )?;
        Ok(self)
    }

    #[cfg(not(unix))]
    fn claiming_passed_sockets(self) -> ConfigResult<Self> {
        Ok(self)
    }

    /// The active app name this handle was loaded for: the instance's, when this process is one.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The app the code named: equal to [`name`](Self::name) unless this process is an instance of it.
    #[must_use]
    pub fn base_name(&self) -> &str {
        &self.base
    }

    /// The loaded configuration.
    #[must_use]
    pub fn config(&self) -> &FundamentalConfig {
        &self.config
    }

    /// The code of the brand this deployment wears (`[apps.<app>] brand`), if any.
    #[must_use]
    pub fn brand(&self) -> Option<&str> {
        self.config
            .apps
            .apps
            .get(&self.name)
            .and_then(|app| app.brand.as_deref())
    }

    /// The deployment environment the configuration was loaded for.
    #[must_use]
    pub fn environment(&self) -> Environment {
        self.config.environment
    }

    /// The configured NATS settings.
    ///
    /// # Errors
    /// [`ConfigError::MissingField`] when `messaging.nats` is not configured for the active app.
    pub fn nats_settings(&self) -> ConfigResult<&NatsSettings> {
        self.config
            .messaging
            .nats
            .as_ref()
            .ok_or_else(|| ConfigError::missing_field("messaging.nats"))
    }

    /// The socket address for the active app's `socket` surface.
    ///
    /// # Errors
    /// [`ConfigError::MissingField`] without the surface or its `port`;
    /// [`ConfigError::InvalidValue`] for an invalid `host`/`port` pair.
    pub fn socket_addr(&self) -> ConfigResult<SocketAddr> {
        self.surface_addr(SOCKET_SURFACE)
    }

    /// The socket address for the active app's `api` surface.
    ///
    /// # Errors
    /// [`ConfigError::MissingField`] without the surface or its `port`;
    /// [`ConfigError::InvalidValue`] for an invalid `host`/`port` pair.
    pub fn api_addr(&self) -> ConfigResult<SocketAddr> {
        self.surface_addr(API_SURFACE)
    }

    /// The socket address for the active app's `web` surface.
    ///
    /// # Errors
    /// [`ConfigError::MissingField`] without the surface or its `port`;
    /// [`ConfigError::InvalidValue`] for an invalid `host`/`port` pair.
    pub fn web_addr(&self) -> ConfigResult<SocketAddr> {
        self.surface_addr(WEB_SURFACE)
    }

    /// The socket address for the active app's `sync` surface.
    ///
    /// # Errors
    /// [`ConfigError::MissingField`] without the surface or its `port`;
    /// [`ConfigError::InvalidValue`] for an invalid `host`/`port` pair.
    pub fn sync_addr(&self) -> ConfigResult<SocketAddr> {
        self.surface_addr(SYNC_SURFACE)
    }

    /// The `dir` override declared on one of the active app's surfaces.
    #[must_use]
    pub fn surface_dir(&self, surface: &str) -> Option<&str> {
        self.config
            .apps
            .apps
            .get(&self.name)
            .and_then(|app| app.surfaces.get(surface))
            .and_then(|entry| entry.dir.as_deref())
    }

    /// The `dir` override declared on the active app's `web` surface.
    #[must_use]
    pub fn web_dir(&self) -> Option<&str> {
        self.surface_dir(WEB_SURFACE)
    }

    /// The directory the web surface serves its bundle from: its `dir`, or else `built`.
    ///
    /// Deployed, a relative `dir` is read from the package the running binary belongs to.
    ///
    /// ```toml
    /// [apps.www.web]
    /// dir = "share/dist"
    /// ```
    ///
    /// # Errors
    /// Deployed: [`ConfigError::BundleUnnamed`] (`CFG_031`) when no `dir` is named;
    /// [`ConfigError::BundleMissing`] (`CFG_032`) when the bundle is not a non-empty directory.
    pub fn web_bundle_dir(&self, built: &str) -> ConfigResult<PathBuf> {
        self.web_bundle_dir_of(built, std::env::current_exe)
    }

    fn web_bundle_dir_of(
        &self,
        built: &str,
        binary: impl FnOnce() -> std::io::Result<PathBuf>,
    ) -> ConfigResult<PathBuf> {
        let deployed = self.environment().is_deployed();
        let named = match self.web_dir() {
            Some(named) if deployed => Path::new(named),
            Some(named) => return Ok(PathBuf::from(named)),
            None if deployed => {
                return Err(ConfigError::BundleUnnamed {
                    app: self.name.clone(),
                    environment: self.environment().to_string(),
                });
            }
            None => return Ok(PathBuf::from(built)),
        };
        let missing = |dir: &Path| ConfigError::BundleMissing {
            app: self.name.clone(),
            dir: dir.display().to_string(),
        };
        let dir = if named.is_absolute() {
            named.to_path_buf()
        } else {
            // `<package>/bin/<name>`; never fall back to the working directory.
            binary()
                .ok()
                .and_then(|binary| Some(binary.parent()?.parent()?.join(named)))
                .ok_or_else(|| missing(named))?
        };
        let holds_a_file =
            std::fs::read_dir(&dir).is_ok_and(|mut entries| entries.next().is_some());
        if !holds_a_file {
            return Err(missing(&dir));
        }
        Ok(dir)
    }

    /// The socket address for the active app's `admin` surface (`/health`, `/ready`, `/metrics`).
    ///
    /// # Errors
    /// [`ConfigError::MissingField`] without the surface or its `port`;
    /// [`ConfigError::InvalidValue`] for an invalid `host`/`port` pair.
    pub fn admin_addr(&self) -> ConfigResult<SocketAddr> {
        self.surface_addr(ADMIN_SURFACE)
    }

    /// Whether the active app should start the named surface; an undeclared surface is enabled.
    #[must_use]
    pub fn surface_enabled(&self, surface: &str) -> bool {
        if let Some(filter) = &self.surfaces_filter {
            return filter.iter().any(|s| s == surface);
        }
        self.config
            .apps
            .apps
            .get(&self.name)
            .and_then(|app| app.surfaces.get(surface))
            .map(|entry| entry.enabled)
            .unwrap_or(true)
    }

    /// These settings running only `surfaces`, as `FUNDAMENTAL_SURFACES` lists them.
    #[must_use]
    pub fn with_surfaces<I, S>(mut self, surfaces: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.surfaces_filter = Some(surfaces.into_iter().map(Into::into).collect());
        self
    }

    /// The surfaces this process was told to run; `None` lets the manifest's `enabled` flags decide.
    #[must_use]
    pub fn named_surfaces(&self) -> Option<&[String]> {
        self.surfaces_filter.as_deref()
    }

    /// Whether the active app declares `surface` (`[apps.<name>.<surface>]`), enabled or not.
    #[must_use]
    pub fn declares_surface(&self, surface: &str) -> bool {
        self.surface_entry(surface).is_ok()
    }

    /// Where one of the active app's network surfaces listens: its address or a passed socket.
    ///
    /// # Errors
    /// [`ConfigError::MissingField`] without the surface or a bound surface's `port`;
    /// [`ConfigError::InvalidValue`] for an invalid address or socket name.
    pub fn surface_listen(&self, surface: &str) -> ConfigResult<Listen> {
        match self.surface_entry(surface)?.listen {
            ListenMode::Tcp => self.surface_addr(surface).map(Listen::Address),
            ListenMode::Activated => SocketName::parse(surface)
                .map(Listen::Activated)
                .ok_or_else(|| {
                    ConfigError::invalid_value(
                        format!("apps.{}.{surface}", self.name),
                        "a surface on a passed socket is named as one: [a-z][a-z0-9_-]{0,63}",
                    )
                }),
        }
    }

    /// Where the active app's `web` surface listens; see [`surface_listen`](Self::surface_listen).
    ///
    /// # Errors
    /// As [`surface_listen`](Self::surface_listen).
    pub fn web_listen(&self) -> ConfigResult<Listen> {
        self.surface_listen(WEB_SURFACE)
    }

    /// Where the active app's `admin` surface listens; see [`surface_listen`](Self::surface_listen).
    ///
    /// # Errors
    /// As [`surface_listen`](Self::surface_listen).
    pub fn admin_listen(&self) -> ConfigResult<Listen> {
        self.surface_listen(ADMIN_SURFACE)
    }

    fn surfaces(&self) -> impl Iterator<Item = (&String, &SurfaceEntryConfig)> {
        self.config
            .apps
            .apps
            .get(&self.name)
            .into_iter()
            .flat_map(|app| app.surfaces.iter())
    }

    fn surface_entry(&self, surface: &str) -> ConfigResult<&SurfaceEntryConfig> {
        self.config
            .apps
            .apps
            .get(&self.name)
            .and_then(|app| app.surfaces.get(surface))
            .ok_or_else(|| ConfigError::missing_field(format!("apps.{}.{surface}", self.name)))
    }

    fn surface_addr(&self, surface: &str) -> ConfigResult<SocketAddr> {
        let settings = self.surface_entry(surface)?;
        if settings.listen == ListenMode::Activated {
            return Err(ConfigError::SurfaceListensActivated {
                surface: format!("apps.{}.{surface}", self.name),
            });
        }

        let port = settings.port.ok_or_else(|| {
            ConfigError::missing_field(format!("apps.{}.{surface}.port", self.name))
        })?;

        let host = settings.host.as_deref().unwrap_or(DEFAULT_SURFACE_HOST);

        format!("{host}:{port}").parse().map_err(|e| {
            ConfigError::invalid_value(
                format!("apps.{}.{surface}", self.name),
                format!("'{host}:{port}' is not a valid socket address: {e}"),
            )
        })
    }
}

/// The `[apps.<key>]` key for `name`: itself, or the one key that spells its `_` as `-`.
fn declared_key(name: &str, toml_path: &Path) -> String {
    let folded = |key: &str| key.replace('-', "_");
    let Some(apps) = std::fs::read_to_string(toml_path)
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("apps").and_then(toml::Value::as_table).cloned())
    else {
        return name.to_owned();
    };
    if apps.contains_key(name) {
        return name.to_owned();
    }
    let mut spelt = apps.keys().filter(|key| folded(key) == folded(name));
    match (spelt.next(), spelt.next()) {
        (Some(key), None) => key.clone(),
        _ => name.to_owned(),
    }
}

/// `None` when unset or empty, meaning every surface runs.
fn parse_surfaces_filter(raw: Option<&str>) -> Option<Vec<String>> {
    let names: Vec<String> = raw?
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if names.is_empty() { None } else { Some(names) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;
    use std::fs;

    struct TempManifest {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
    }

    impl TempManifest {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    fn write_toml(contents: &str) -> TempManifest {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("fun.toml");
        fs::write(&path, contents).expect("write toml");
        TempManifest { _dir: dir, path }
    }

    #[test]
    fn load_from_reads_environment_and_name() {
        let toml = write_toml(
            r#"
            [apps.main]
            socket = { host = "127.0.0.1", port = 9000 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load main app");
        assert_eq!(app.name(), "main");
        assert!(app.environment().is_development());
    }

    #[test]
    fn a_crate_name_finds_the_app_its_hyphens_spell() {
        let toml = write_toml(
            r#"
            [apps.my-svc]
            web = { port = 9000 }
            "#,
        );

        let app = AppSettings::load_from("my_svc", toml.path()).expect("load my-svc");
        assert_eq!(app.name(), "my-svc");
        assert_eq!(app.web_addr().expect("its web address").port(), 9000);
    }

    #[test]
    fn an_app_reads_the_brand_it_names_and_an_instance_its_own() {
        let toml = write_toml(
            r#"
            [apps.www]
            brand = "dagen"
            web = { port = 9000 }

            [apps.www_days]
            instance_of = "www"
            brand = "days"

            [apps.mijn]
            web = { port = 9001 }
            "#,
        );

        let www = AppSettings::load_from("www", toml.path()).expect("load www");
        assert_eq!(www.brand(), Some("dagen"));
        let days =
            AppSettings::load_instance_from("www", "www_days", toml.path()).expect("load www_days");
        assert_eq!(days.brand(), Some("days"));
        let mijn = AppSettings::load_from("mijn", toml.path()).expect("load mijn");
        assert_eq!(mijn.brand(), None, "an app that names no brand wears none");
    }

    #[test]
    fn a_brand_that_is_not_a_code_is_refused() {
        let toml = write_toml(
            r#"
            [apps.www]
            brand = ["dagen"]
            "#,
        );
        let error = AppSettings::load_from("www", toml.path()).unwrap_err();
        assert!(
            matches!(&error, ConfigError::InvalidValue { field, .. } if field == "apps.www.brand"),
            "{error}"
        );
    }

    #[test]
    fn an_apps_own_identity_section_is_not_the_instance_identity_list() {
        let toml = write_toml(
            r#"
            [apps.business]
            web = { port = 9000 }
            instance_identity = ["identity.session_signing_key_id"]

            [apps.business.identity]
            session_ttl_secs = 3600
            session_signing_key_id = "business-1"
            "#,
        );
        let business = AppSettings::load_from("business", toml.path()).expect("load business");
        let identity: toml::Value = business
            .config()
            .custom
            .section("identity")
            .expect("the app's own section");
        assert_eq!(identity["session_ttl_secs"].as_integer(), Some(3600));
    }

    #[test]
    fn load_rejects_a_deployed_config_that_serves_plaintext() {
        let toml = write_toml(
            r#"
            [apps.main]
            api = { host = "0.0.0.0", port = 9000 }
            "#,
        );

        let error = AppSettings::load_in(
            "main",
            toml.path(),
            fundamental_core::Environment::Production,
        )
        .expect_err("plaintext in production");

        assert_eq!(error.error_code(), "CFG_010");
        let message = error.display_message();
        assert!(
            message.contains("TLS"),
            "the reason must be in the message, not just a count: {message}"
        );
    }

    #[test]
    fn load_accepts_a_deployed_config_whose_tls_terminates_upstream() {
        let toml = write_toml(
            r#"
            [server]
            tls = "terminated_upstream"

            [apps.main]
            api = { host = "0.0.0.0", port = 9000, tls = "terminated_upstream" }
            "#,
        );

        let app = AppSettings::load_in(
            "main",
            toml.path(),
            fundamental_core::Environment::Production,
        )
        .expect("a proxied service is valid");
        assert!(app.environment().is_deployed());
    }

    #[test]
    fn an_api_only_app_is_not_asked_about_a_web_server_it_never_starts() {
        let toml = write_toml(
            r#"
            [apps.main]
            api = { host = "0.0.0.0", port = 9000, tls = "terminated_upstream" }
            "#,
        );

        let app = AppSettings::load_in(
            "main",
            toml.path(),
            fundamental_core::Environment::Production,
        )
        .expect("an API-only service has no web listener to secure");

        assert!(app.environment().is_deployed());
    }

    #[test]
    fn a_web_app_is_still_asked_about_its_web_server() {
        let toml = write_toml(
            r#"
            [apps.main]
            web = { host = "0.0.0.0", port = 9000 }
            "#,
        );

        let error = AppSettings::load_in(
            "main",
            toml.path(),
            fundamental_core::Environment::Production,
        )
        .expect_err("a declared web listener must declare its posture");

        assert_eq!(error.error_code(), "CFG_010");
        assert!(error.display_message().contains("Server TLS"));
    }

    #[test]
    fn a_surface_dir_override_is_readable_and_absent_by_default() {
        let toml = write_toml(
            r#"
            [apps.main]
            web = { host = "127.0.0.1", port = 9000, dir = "/srv/wallet/dist" }
            api = { host = "127.0.0.1", port = 9001 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");

        assert_eq!(app.web_dir(), Some("/srv/wallet/dist"));
        assert_eq!(
            app.surface_dir("api"),
            None,
            "no override means the caller keeps its own default"
        );
        assert_eq!(app.surface_dir("nonexistent"), None);
    }

    #[test]
    fn a_web_bundle_in_development_is_the_built_one_unless_named() {
        let unnamed = write_toml(
            r#"
            [apps.main]
            web = { host = "127.0.0.1", port = 9000 }
            "#,
        );
        let named = write_toml(
            r#"
            [apps.main]
            web = { host = "127.0.0.1", port = 9000, dir = "share/dist" }
            "#,
        );

        let unnamed = AppSettings::load_from("main", unnamed.path()).expect("load app");
        let named = AppSettings::load_from("main", named.path()).expect("load app");

        assert_eq!(
            unnamed.web_bundle_dir("/build/www/dist").ok(),
            Some(PathBuf::from("/build/www/dist"))
        );
        assert_eq!(
            named.web_bundle_dir("/build/www/dist").ok(),
            Some(PathBuf::from("share/dist"))
        );
    }

    fn deployed_web(dir: Option<&str>) -> AppSettings {
        let dir = dir.map_or(String::new(), |dir| format!(", dir = \"{dir}\""));
        let toml = write_toml(&format!(
            "[apps.main]\nweb = {{ host = \"127.0.0.1\", port = 9000, tls = \"terminated_upstream\"{dir} }}\n"
        ));
        AppSettings::load_in(
            "main",
            toml.path(),
            fundamental_core::Environment::Production,
        )
        .expect("load app")
    }

    /// A package as `fun package` lays it out: `bin/` and `share/dist` holding one file.
    fn package() -> tempfile::TempDir {
        let package = tempfile::tempdir().expect("a package");
        fs::create_dir_all(package.path().join("bin")).expect("bin/");
        fs::create_dir_all(package.path().join("share/dist")).expect("share/dist");
        fs::write(package.path().join("share/dist/app.css"), "body{}").expect("a bundle file");
        package
    }

    #[test]
    fn a_deployed_relative_bundle_is_read_from_the_package_the_binary_runs_from() {
        let package = package();
        let binary = package.path().join("bin/www");

        let dir = deployed_web(Some("share/dist"))
            .web_bundle_dir_of("/build/www/dist", || Ok(binary.clone()))
            .expect("the package's bundle");

        assert_eq!(dir, package.path().join("share/dist"));
    }

    #[test]
    fn a_deployed_web_bundle_left_unnamed_is_refused() {
        let refused = deployed_web(None)
            .web_bundle_dir("/build/www/dist")
            .expect_err("a deployed bundle must be named");

        assert_eq!(refused.error_code(), "CFG_031");
        assert!(refused.display_message().contains("main"), "{refused}");
    }

    #[test]
    fn a_deployed_web_bundle_that_is_missing_or_empty_is_refused_naming_it() {
        let package = package();
        let binary = package.path().join("bin/www");
        fs::create_dir_all(package.path().join("share/empty")).expect("an empty directory");

        for named in ["share/missing", "share/empty"] {
            let refused = deployed_web(Some(named))
                .web_bundle_dir_of("/build/www/dist", || Ok(binary.clone()))
                .expect_err("no bundle there");

            assert_eq!(refused.error_code(), "CFG_032", "{named}");
            assert!(
                refused
                    .display_message()
                    .contains(&package.path().join(named).display().to_string()),
                "{refused}"
            );
        }
    }

    #[test]
    fn a_deployed_relative_bundle_whose_package_cannot_be_found_is_refused() {
        let refused = deployed_web(Some("src"))
            .web_bundle_dir_of("/build/www/dist", || {
                Err(std::io::Error::other("no binary"))
            })
            .expect_err("no package to read it from");

        assert_eq!(refused.error_code(), "CFG_032");
    }

    #[test]
    fn a_deployed_absolute_bundle_is_taken_as_named() {
        let package = package();
        let absolute = package.path().join("share/dist");
        let absolute = absolute.to_str().expect("a UTF-8 path");

        let dir = deployed_web(Some(absolute))
            .web_bundle_dir_of("/build/www/dist", || Err(std::io::Error::other("unused")))
            .expect("the named bundle");

        assert_eq!(dir, PathBuf::from(absolute));
    }

    #[test]
    fn load_leaves_development_alone() {
        let toml = write_toml(
            r#"
            [apps.main]
            api = { host = "127.0.0.1", port = 9000 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("development needs no TLS");
        assert!(app.environment().is_development());
    }

    #[test]
    fn nats_settings_returns_configured_nats() {
        let toml = write_toml(
            r#"
            [apps.main.messaging.nats]
            url = "nats://example:4222"
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let nats = app.nats_settings().expect("nats configured");
        assert_eq!(nats.url, "nats://example:4222");
    }

    #[test]
    fn nats_settings_errors_when_not_configured() {
        let toml = write_toml(
            r#"
            [apps.main]
            socket = { port = 9000 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let err = app.nats_settings().expect_err("nats not configured");
        assert!(
            matches!(&err, ConfigError::MissingField { field } if field == "messaging.nats"),
            "expected MissingField for messaging.nats, got {err:?}"
        );
    }

    #[test]
    fn socket_addr_parses_host_and_port() {
        let toml = write_toml(
            r#"
            [apps.main]
            socket = { host = "0.0.0.0", port = 9100 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let addr = app.socket_addr().expect("socket addr");
        assert_eq!(addr.to_string(), "0.0.0.0:9100");
    }

    #[test]
    fn socket_addr_defaults_host_to_loopback() {
        let toml = write_toml(
            r#"
            [apps.main]
            socket = { port = 9200 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let addr = app.socket_addr().expect("socket addr");
        assert_eq!(addr.to_string(), "127.0.0.1:9200");
    }

    #[test]
    fn socket_addr_errors_when_port_missing() {
        let toml = write_toml(
            r#"
            [apps.main]
            socket = { host = "127.0.0.1" }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let err = app.socket_addr().expect_err("port missing");
        assert!(
            matches!(&err, ConfigError::MissingField { field } if field == "apps.main.socket.port"),
            "expected MissingField for the missing port, got {err:?}"
        );
    }

    #[test]
    fn socket_addr_errors_when_surface_missing() {
        let toml = write_toml(
            r#"
            [apps.main.messaging.nats]
            url = "nats://localhost:4222"
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let err = app.socket_addr().expect_err("no socket surface");
        assert!(
            matches!(&err, ConfigError::MissingField { field } if field == "apps.main.socket"),
            "expected MissingField for the missing surface, got {err:?}"
        );
    }

    #[test]
    fn api_addr_parses_host_and_port() {
        let toml = write_toml(
            r#"
            [apps.main]
            api = { host = "0.0.0.0", port = 16001 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let addr = app.api_addr().expect("api addr");
        assert_eq!(addr.to_string(), "0.0.0.0:16001");
    }

    #[test]
    fn api_addr_defaults_host_to_loopback() {
        let toml = write_toml(
            r#"
            [apps.main]
            api = { port = 16001 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let addr = app.api_addr().expect("api addr");
        assert_eq!(addr.to_string(), "127.0.0.1:16001");
    }

    #[test]
    fn web_addr_parses_host_and_port() {
        let toml = write_toml(
            r#"
            [apps.main]
            web = { host = "0.0.0.0", port = 11001 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let addr = app.web_addr().expect("web addr");
        assert_eq!(addr.to_string(), "0.0.0.0:11001");
    }

    #[test]
    fn web_addr_errors_when_surface_missing() {
        let toml = write_toml(
            r#"
            [apps.main]
            api = { port = 16001 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let err = app.web_addr().expect_err("no web surface");
        assert!(
            matches!(&err, ConfigError::MissingField { field } if field == "apps.main.web"),
            "expected MissingField for the missing web surface, got {err:?}"
        );
    }

    #[test]
    fn api_addr_errors_when_surface_missing() {
        let toml = write_toml(
            r#"
            [apps.main]
            socket = { port = 9000 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let err = app.api_addr().expect_err("no api surface");
        assert!(
            matches!(&err, ConfigError::MissingField { field } if field == "apps.main.api"),
            "expected MissingField for the missing api surface, got {err:?}"
        );
    }

    #[test]
    fn parse_surfaces_filter_handles_empty_and_lists() {
        assert_eq!(parse_surfaces_filter(None), None);
        assert_eq!(parse_surfaces_filter(Some("")), None);
        assert_eq!(parse_surfaces_filter(Some("   ")), None);
        assert_eq!(parse_surfaces_filter(Some(",, ,")), None);
        assert_eq!(
            parse_surfaces_filter(Some("web")),
            Some(vec!["web".to_string()])
        );
        assert_eq!(
            parse_surfaces_filter(Some(" web , api ")),
            Some(vec!["web".to_string(), "api".to_string()])
        );
    }

    #[test]
    fn surface_enabled_defaults_to_true_and_honors_the_config_flag() {
        let toml = write_toml(
            r#"
            [apps.main]
            worker = {}
            api = { port = 16001, enabled = false }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        assert!(app.surface_enabled("worker"), "default-enabled surface");
        assert!(!app.surface_enabled("api"), "explicitly disabled surface");
        assert!(app.surface_enabled("web"), "undeclared surface");
    }

    #[test]
    fn surface_enabled_allowlist_overrides_the_config() {
        let toml = write_toml(
            r#"
            [apps.main]
            worker = {}
            api = { port = 16001 }
            web = { port = 11001 }
            "#,
        );

        let mut app = AppSettings::load_from("main", toml.path()).expect("load app");
        app.surfaces_filter = Some(vec!["web".to_string()]);
        assert!(app.surface_enabled("web"));
        assert!(!app.surface_enabled("worker"));
        assert!(!app.surface_enabled("api"));
    }

    #[test]
    fn a_process_names_the_surfaces_it_was_given_and_none_when_given_none() {
        let toml = write_toml("[apps.main]\nworker = {}\n");
        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        assert_eq!(app.named_surfaces(), None);
        let app = app.with_surfaces(["worker", "admin"]);
        assert_eq!(
            app.named_surfaces(),
            Some(&["worker".to_owned(), "admin".to_owned()][..])
        );
    }

    #[test]
    fn a_process_given_its_surfaces_runs_those_and_knows_what_is_declared() {
        let toml = write_toml(
            r#"
            [apps.main]
            worker = {}
            web = { port = 11001 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path())
            .expect("load app")
            .with_surfaces(["worker"]);
        assert!(app.surface_enabled("worker"));
        assert!(!app.surface_enabled("web"));
        assert!(app.declares_surface("web"));
        assert!(!app.declares_surface("admin"));
    }

    #[test]
    fn an_activated_surface_listens_on_the_socket_passed_under_its_name() {
        let toml = write_toml(
            r#"
            [apps.main]
            web = { listen = "activated" }
            admin = { port = 9100 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path())
            .expect("a process passed no socket loads, and is refused where it binds");

        assert_eq!(
            app.web_listen().expect("web listens"),
            Listen::Activated(SocketName::parse("web").expect("a socket name"))
        );
        assert_eq!(
            app.admin_listen().expect("admin listens"),
            Listen::Address("127.0.0.1:9100".parse().expect("an address"))
        );
    }

    #[test]
    fn an_activated_surface_has_no_address_to_bind() {
        let toml = write_toml(
            r#"
            [apps.main]
            web = { listen = "activated", port = 11001 }
            "#,
        );

        let app = AppSettings::load_from("main", toml.path()).expect("load app");
        let err = app
            .web_addr()
            .expect_err("an activated surface binds nothing");

        assert!(
            matches!(&err, ConfigError::SurfaceListensActivated { surface } if surface == "apps.main.web"),
            "{err:?}"
        );
        assert_eq!(err.error_code(), "CFG_025");
        assert_eq!(err.http_status_code(), 500);
    }

    #[test]
    fn a_listen_mode_other_than_tcp_or_activated_is_refused() {
        let toml = write_toml(
            r#"
            [apps.main]
            web = { listen = "inherited", port = 11001 }
            "#,
        );

        assert!(AppSettings::load_from("main", toml.path()).is_err());
    }
    #[test]
    fn a_release_loaded_outside_a_deployed_environment_is_refused() {
        let toml = write_toml(
            r#"
            [apps.main]
            web = { port = 9000 }
            "#,
        );

        let error = AppSettings::load_release("main", toml.path()).unwrap_err();

        assert_eq!(error.error_code(), "CFG_030");
        assert!(error.display_message().contains("development"), "{error}");
    }

    #[test]
    fn a_release_is_loaded_without_the_sockets_its_service_is_passed() {
        let toml = write_toml(
            r#"
            [server]
            tls = "terminated_upstream"

            [apps.main]
            web = { listen = "activated", tls = "terminated_upstream" }
            "#,
        );

        let app = AppSettings::load_release_with(
            "main",
            toml.path(),
            ConfigLoader::new(fundamental_core::Environment::Production),
        )
        .expect("a release whose web socket is passed at boot");

        assert!(app.environment().is_production());
    }
}

/// The file that overrides `fun.toml`, and its place in the order.
#[cfg(test)]
mod env_file_layer_tests {
    use super::*;
    use fundamental_core::Environment;
    use fundamental_core::env::EnvVars;
    use std::fs;
    use tempfile::TempDir;

    /// A project directory: its `fun.toml`, and this machine's `fun.local.toml` beside it.
    fn project() -> TempDir {
        let dir = TempDir::new().expect("temp project");
        fs::write(
            dir.path().join("fun.toml"),
            r#"
            [custom.origins]
            www = "https://base.example"
            business = "https://business.example"

            [apps.main]
            socket = { host = "127.0.0.1", port = 9000 }
            admin = { host = "127.0.0.1", port = 9001 }

            [apps.main.federation]
            entity_id = "https://main.example"
            "#,
        )
        .expect("write manifest");
        fs::write(
            dir.path().join("fun.local.toml"),
            r#"
            [custom.origins]
            www = "https://from-env-file.example"
            "#,
        )
        .expect("write the machine's file");
        dir
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_root_reads_its_environments_file_from_its_release() {
        let release = project();
        fs::write(
            release.path().join("fun.production.toml"),
            "[custom.origins]\nwww = \"https://from-the-release.example\"\n",
        )
        .expect("write the release's production file");
        let home = tempfile::tempdir().expect("create temp dir");
        std::os::unix::fs::symlink(
            release.path().join("fun.toml"),
            home.path().join("fun.toml"),
        )
        .expect("link the manifest");
        fs::write(
            home.path().join("fun.production.toml"),
            "[custom.origins]\nwww = \"https://beside-the-link.example\"\n",
        )
        .expect("write a stray production file");
        let app = AppSettings::load_in(
            "main",
            home.path().join("fun.toml"),
            Environment::Production,
        )
        .expect("the linked manifest loads");
        assert_eq!(
            app.config().custom.get_str("origins.www"),
            Some("https://from-the-release.example"),
            "the environment's file comes from the release the link resolves to"
        );
    }

    #[test]
    fn a_deployment_boots_an_instance_of_the_binarys_app() {
        let dir = project();
        fs::write(
            dir.path().join("fun.toml"),
            format!(
                "{}\n[apps.main_sandbox]\ninstance_of = \"main\"\n[apps.main_sandbox.federation]\nentity_id = \"https://sandbox.example\"\n",
                fs::read_to_string(dir.path().join("fun.toml")).expect("read manifest")
            ),
        )
        .expect("write manifest");
        let app = AppSettings::load_in_reading_env_from(
            "main",
            dir.path().join("fun.toml"),
            Environment::Development,
            EnvVars::fixed([("FUNDAMENTAL_INSTANCE", "main_sandbox")]),
        )
        .expect("the instance loads");
        assert_eq!(app.name(), "main_sandbox");
        assert_eq!(app.base_name(), "main");
        assert_eq!(
            app.config().custom.get_str("federation.entity_id"),
            Some("https://sandbox.example"),
            "the instance's own section"
        );
        assert_eq!(
            app.admin_addr().expect("an admin listener").port(),
            9001,
            "inherited from the base"
        );

        let base = AppSettings::load_in_reading_env_from(
            "main",
            dir.path().join("fun.toml"),
            Environment::Development,
            EnvVars::fixed([("FUNDAMENTAL_ENV", "development")]),
        )
        .expect("the base app loads beside a declared instance of it");
        assert_eq!(base.name(), "main");
        assert_eq!(
            base.config().custom.get_str("federation.entity_id"),
            Some("https://main.example"),
            "the base's own section, not its instance's"
        );

        let refused = AppSettings::load_in_reading_env_from(
            "main",
            dir.path().join("fun.toml"),
            Environment::Development,
            EnvVars::fixed([("FUNDAMENTAL_INSTANCE", "main_staging")]),
        );
        assert!(matches!(
            refused,
            Err(ConfigError::AppInstanceUndeclared { .. })
        ));
    }

    #[test]
    fn a_runtime_variable_sets_an_instance_key_its_base_leaves_unset() {
        let dir = project();
        fs::write(
            dir.path().join("fun.toml"),
            format!(
                "{}\n[apps.main.web]\nport = 3000\n[apps.main_sandbox]\ninstance_of = \"main\"\nweb = {{ port = 3001 }}\n",
                fs::read_to_string(dir.path().join("fun.toml")).expect("read manifest")
            ),
        )
        .expect("write manifest");
        let app = AppSettings::load_in_reading_env_from(
            "main",
            dir.path().join("fun.toml"),
            Environment::Development,
            EnvVars::fixed([
                ("FUNDAMENTAL_INSTANCE", "main_sandbox"),
                ("FUNDAMENTAL_APPS__MAIN_SANDBOX__WEB__HOST", "127.0.0.9"),
                ("FUNDAMENTAL_APPS__MAIN_SANDBOX__ADMIN__HOST", "127.0.0.9"),
            ]),
        )
        .expect("a runtime variable is not a restatement the base must have");
        assert_eq!(
            app.web_addr().expect("a web listener").to_string(),
            "127.0.0.9:3001"
        );
        assert_eq!(
            app.admin_addr().expect("an admin listener").to_string(),
            "127.0.0.9:9001",
            "the base's port, the variable's host"
        );

        fs::write(
            dir.path().join("fun.toml"),
            fs::read_to_string(dir.path().join("fun.toml"))
                .expect("read manifest")
                .replace(
                    "web = { port = 3001 }",
                    "web = { port = 3001, hots = \"127.0.0.9\" }",
                ),
        )
        .expect("write manifest");
        let misspelt = AppSettings::load_in_reading_env_from(
            "main",
            dir.path().join("fun.toml"),
            Environment::Development,
            EnvVars::fixed([("FUNDAMENTAL_INSTANCE", "main_sandbox")]),
        );
        assert!(
            matches!(
                misspelt,
                Err(ConfigError::AppInstanceRestatesUnknownKey { ref key, .. }) if key == "web.hots"
            ),
            "the manifest is judged as before: {misspelt:?}"
        );
    }

    #[test]
    fn an_instance_variable_beyond_a_listener_address_is_judged_with_the_manifest() {
        let dir = project();
        fs::write(
            dir.path().join("fun.toml"),
            format!(
                "{}\n[apps.main.database]\nurl = \"${{file:secrets/main}}\"\n[apps.main_sandbox]\ninstance_of = \"main\"\n[apps.main_sandbox.database]\nurl = \"${{file:secrets/sandbox}}\"\n",
                fs::read_to_string(dir.path().join("fun.toml")).expect("read manifest")
            ),
        )
        .expect("write manifest");
        let load = |variable: &'static str, value: &'static str| {
            AppSettings::load_in_reading_env_from(
                "main",
                dir.path().join("fun.toml"),
                Environment::Development,
                EnvVars::fixed([("FUNDAMENTAL_INSTANCE", "main_sandbox"), (variable, value)]),
            )
        };

        let secret = load(
            "FUNDAMENTAL_APPS__MAIN_SANDBOX__DATABASE__URL",
            "${file:secrets/main}",
        );
        assert!(
            matches!(
                secret,
                Err(ConfigError::AppInstanceInheritsSecret { ref key, .. }) if key == "database.url"
            ),
            "the base's secret, handed back by a variable: {secret:?}"
        );
        let section = load("FUNDAMENTAL_APPS__MAIN_SANDBOX__VAULT__ROLE", "main");
        assert!(
            matches!(
                section,
                Err(ConfigError::AppInstanceRestatesUnknownKey { ref key, .. }) if key == "vault"
            ),
            "a section the base lacks: {section:?}"
        );
        let misspelt = load("FUNDAMENTAL_APPS__MAIN_SANDBOX__ADMIN__HOTS", "127.0.0.9");
        assert!(
            matches!(
                misspelt,
                Err(ConfigError::AppInstanceRestatesUnknownKey { ref key, .. }) if key == "admin.hots"
            ),
            "a misspelt listener key: {misspelt:?}"
        );
    }

    #[test]
    fn a_named_instance_loads_whatever_instance_the_process_is() {
        let dir = project();
        fs::write(
            dir.path().join("fun.toml"),
            format!(
                "{}\n[apps.main_sandbox]\ninstance_of = \"main\"\n[apps.main_sandbox.federation]\nentity_id = \"https://sandbox.example\"\n[apps.other]\nadmin = {{ port = 9002 }}\n[apps.other_sandbox]\ninstance_of = \"other\"\n",
                fs::read_to_string(dir.path().join("fun.toml")).expect("read manifest")
            ),
        )
        .expect("write manifest");
        let peer = |instance: &str| {
            AppSettings::load_resolved(
                "main",
                dir.path().join("fun.toml"),
                ConfigLoader::new(Environment::Development)
                    .reading_env_from(EnvVars::fixed([("FUNDAMENTAL_INSTANCE", "other_sandbox")]))
                    .with_instance(instance),
            )
        };

        let sandbox = peer("main_sandbox").expect("the named instance loads");
        assert_eq!(sandbox.name(), "main_sandbox");
        assert_eq!(sandbox.base_name(), "main");
        assert_eq!(
            sandbox.config().custom.get_str("federation.entity_id"),
            Some("https://sandbox.example")
        );

        let base = peer("main").expect("naming the base loads the base");
        assert_eq!(base.name(), "main");
        assert_eq!(
            base.config().custom.get_str("federation.entity_id"),
            Some("https://main.example")
        );

        assert!(matches!(
            peer("other_sandbox"),
            Err(ConfigError::AppInstanceOfAnotherApp { .. })
        ));
        assert!(matches!(
            peer("main_staging"),
            Err(ConfigError::AppInstanceUndeclared { .. })
        ));
    }

    fn load(dir: &TempDir) -> AppSettings {
        AppSettings::load_in(
            "main",
            dir.path().join("fun.toml"),
            Environment::Development,
        )
        .expect("load main app")
    }

    #[test]
    fn a_per_environment_file_layers_over_the_manifest() {
        let dir = project();
        let app = load(&dir);
        let custom = &app.config().custom;

        assert_eq!(
            custom.get_str("origins.www"),
            Some("https://from-env-file.example"),
            "fun.local.toml overrides fun.toml in development"
        );
        assert_eq!(
            custom.get_str("origins.business"),
            Some("https://business.example"),
            "and the key it does not restate survives"
        );
    }

    /// An unrecognised surface such as `admin` would synthesise a `custom` table replacing the root one.
    #[test]
    fn a_root_custom_section_and_an_app_section_both_arrive() {
        let dir = project();
        let app = load(&dir);
        let custom = &app.config().custom;

        assert!(
            custom.get_str("origins.www").is_some(),
            "the project-wide section survived promotion: {:?}",
            custom.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            custom.get_str("federation.entity_id"),
            Some("https://main.example"),
            "and the app's own section is beside it"
        );
        assert!(
            custom.get("admin.port").is_none(),
            "the admin listener is a surface, not a custom section"
        );
    }

    /// Precedence is asserted with the environment as a value, so no test races another.
    #[test]
    fn an_environment_variable_still_outranks_the_file() {
        let dir = project();
        let loaded = AppSettings::load_in_reading_env_from(
            "main",
            dir.path().join("fun.toml"),
            Environment::Development,
            EnvVars::fixed([(
                "FUNDAMENTAL_CUSTOM__ORIGINS__WWW",
                "https://from-env-var.example",
            )]),
        )
        .expect("load main app")
        .config()
        .custom
        .get_str("origins.www")
        .map(str::to_owned);

        assert_eq!(
            loaded.as_deref(),
            Some("https://from-env-var.example"),
            "the environment variable is the outermost layer"
        );
    }

    #[test]
    fn a_runtime_variable_has_an_instance_listen_on_a_passed_socket() {
        let dir = project();
        fs::write(
            dir.path().join("fun.toml"),
            format!(
                "{}\n[apps.main.web]\nport = 3000\n[apps.main_sandbox]\ninstance_of = \"main\"\nweb = {{ port = 3001 }}\n",
                fs::read_to_string(dir.path().join("fun.toml")).expect("read manifest")
            ),
        )
        .expect("write manifest");

        let app = AppSettings::load_in_reading_env_from(
            "main",
            dir.path().join("fun.toml"),
            Environment::Development,
            EnvVars::fixed([
                ("FUNDAMENTAL_INSTANCE", "main_sandbox"),
                ("FUNDAMENTAL_APPS__MAIN_SANDBOX__WEB__LISTEN", "activated"),
            ]),
        )
        .expect("where a listener comes from is set like where it binds");

        assert_eq!(
            app.web_listen().expect("web listens"),
            Listen::Activated(SocketName::parse("web").expect("a socket name"))
        );
    }
}
