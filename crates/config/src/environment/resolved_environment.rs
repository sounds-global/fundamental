//! [`ResolvedEnvironment`]: the environment a process runs as, decided.

use super::{EnvironmentOrigin, resolve_environment};
use crate::ConfigResult;
use crate::source::project_manifest;

use fundamental_core::env::EnvVars;
use fundamental_core::{BuildProfile, Environment, EnvironmentPosture};

use std::path::Path;
use tracing::instrument;

/// The environment a process runs as, as [`resolve_environment`] decided it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEnvironment {
    environment: Environment,
    name: Option<String>,
    origin: EnvironmentOrigin,
}

impl ResolvedEnvironment {
    pub(crate) fn new(
        environment: Environment,
        name: Option<String>,
        origin: EnvironmentOrigin,
    ) -> Self {
        Self {
            environment,
            name,
            origin,
        }
    }

    /// Resolves the environment `vars` names against the `fun.toml` at `manifest`, before expansion.
    ///
    /// # Errors
    /// [`ConfigError::Manifest`](crate::ConfigError::Manifest),
    /// [`ConfigError::NotAManifest`](crate::ConfigError::NotAManifest), or what
    /// [`resolve_environment`] refuses.
    #[instrument(skip(vars, manifest), fields(manifest = %manifest.as_ref().display(), %profile), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn for_manifest(
        vars: &EnvVars,
        manifest: impl AsRef<Path>,
        profile: BuildProfile,
    ) -> ConfigResult<Self> {
        let manifest = project_manifest(manifest.as_ref(), false)?
            .map(|manifest| toml::Value::Table(manifest.as_table().clone()));
        resolve_environment(
            EnvironmentOrigin::from_vars(vars),
            manifest.as_ref(),
            profile,
        )
    }

    /// The built-in environment whose rules apply.
    #[must_use]
    pub const fn environment(&self) -> Environment {
        self.environment
    }

    /// The custom `[environments.*]` table read for this environment, when not a built-in's own.
    #[must_use]
    pub fn custom_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The name this environment is known by: its custom name, or the built-in's.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name
            .as_deref()
            .unwrap_or_else(|| self.environment.as_str())
    }

    /// The posture this environment is held to.
    #[must_use]
    pub const fn posture(&self) -> EnvironmentPosture {
        self.environment.posture()
    }

    /// Where the environment came from.
    #[must_use]
    pub const fn origin(&self) -> &EnvironmentOrigin {
        &self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConfigError;

    use tempfile::TempDir;

    #[test]
    fn a_manifest_is_read_as_written_before_any_expansion() {
        let dir = TempDir::new().expect("tempdir");
        let manifest = dir.path().join("fun.toml");
        std::fs::write(
            &manifest,
            "[environments.devops]\nposture = \"${POSTURE}\"\n",
        )
        .expect("write manifest");
        let vars = EnvVars::fixed([(Environment::ENV_VAR, "devops"), ("POSTURE", "development")]);

        let err = ResolvedEnvironment::for_manifest(&vars, &manifest, BuildProfile::Release)
            .expect_err("an expansion is not a declaration");
        assert!(matches!(err, ConfigError::InvalidValue { .. }), "{err:?}");
    }

    #[test]
    fn a_missing_manifest_declares_nothing() {
        let dir = TempDir::new().expect("tempdir");
        let vars = EnvVars::fixed([(Environment::ENV_VAR, "devops")]);

        let err = ResolvedEnvironment::for_manifest(
            &vars,
            dir.path().join("fun.toml"),
            BuildProfile::Release,
        )
        .expect_err("undeclared in a release build");
        assert!(matches!(
            err,
            ConfigError::EnvironmentPostureUndeclared { .. }
        ));
    }

    #[test]
    fn a_declared_custom_name_keeps_its_name_and_enforces_its_posture() {
        let dir = TempDir::new().expect("tempdir");
        let manifest = dir.path().join("fun.toml");
        std::fs::write(&manifest, "[environments.devops]\nposture = \"deployed\"\n")
            .expect("write manifest");
        let vars = EnvVars::fixed([(Environment::ENV_VAR, "devops")]);

        let resolved = ResolvedEnvironment::for_manifest(&vars, &manifest, BuildProfile::Release)
            .expect("declared");
        assert_eq!(resolved.environment(), Environment::Production);
        assert_eq!(resolved.custom_name(), Some("devops"));
        assert_eq!(resolved.name(), "devops");
        assert_eq!(resolved.posture(), EnvironmentPosture::Deployed);
        assert_eq!(
            resolved.origin(),
            &EnvironmentOrigin::Variable("devops".to_owned())
        );
    }
}
