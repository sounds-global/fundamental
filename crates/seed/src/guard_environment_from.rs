use crate::environment_guard::override_enabled;
use crate::{RELEASE_OVERRIDE_VAR, SeedError, guard_environment};

use fundamental_config::ResolvedEnvironment;
use fundamental_core::BuildProfile;
use fundamental_core::env::EnvVars;

use std::path::Path;
use tracing::instrument;

/// Resolves the environment `vars` names against the `fun.toml` at `manifest`, with the
/// config loader's own resolver, then applies [`guard_environment`].
///
/// # Errors
///
/// - [`SeedError::Config`] — the environment could not be resolved.
/// - [`SeedError::EnvironmentNotAllowed`] — a deployed environment without the override.
#[instrument(skip(vars), fields(manifest = %manifest.as_ref().display(), %profile), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub fn guard_environment_from(
    vars: &EnvVars,
    manifest: impl AsRef<Path>,
    profile: BuildProfile,
) -> Result<ResolvedEnvironment, SeedError> {
    let resolved = ResolvedEnvironment::for_manifest(vars, manifest, profile)?;
    let allow = override_enabled(vars.get(RELEASE_OVERRIDE_VAR).as_deref());
    guard_environment(&resolved, allow)?;
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_config::ConfigLoader;
    use fundamental_core::{Environment, Error as _};

    use std::path::PathBuf;
    use tempfile::TempDir;

    const APP: &str = "[apps.main]\nweb = { port = 1 }\n";

    struct Row {
        label: &'static str,
        variable: Option<&'static str>,
        declares: &'static str,
    }

    const ROWS: [Row; 6] = [
        Row {
            label: "unset",
            variable: None,
            declares: "",
        },
        Row {
            label: "built-in",
            variable: Some("production"),
            declares: "",
        },
        Row {
            label: "alias",
            variable: Some("prod"),
            declares: "",
        },
        Row {
            label: "declared deployed",
            variable: Some("devops"),
            declares: "[environments.devops]\nposture = \"deployed\"\n",
        },
        Row {
            label: "declared development",
            variable: Some("laptop"),
            declares: "[environments.laptop]\nposture = \"development\"\n",
        },
        Row {
            label: "undeclared",
            variable: Some("producton"),
            declares: "",
        },
    ];

    type Outcome = Result<(Environment, String), &'static str>;

    fn manifest(dir: &TempDir, row: &Row) -> PathBuf {
        let path = dir.path().join("fun.toml");
        std::fs::write(&path, format!("{APP}{}", row.declares)).expect("write manifest");
        path
    }

    fn vars(row: &Row, overridden: bool) -> EnvVars {
        let mut pairs = Vec::new();
        if let Some(value) = row.variable {
            pairs.push((Environment::ENV_VAR, value));
        }
        if overridden {
            pairs.push((RELEASE_OVERRIDE_VAR, "1"));
        }
        EnvVars::fixed(pairs)
    }

    fn loader_decides(row: &Row, path: &Path, profile: BuildProfile) -> Outcome {
        let mut loader = ConfigLoader::auto_from(vars(row, false)).with_fundamental_toml(path);
        if profile == BuildProfile::Release {
            loader = loader.as_release_build();
        }
        loader
            .load()
            .map(|config| (config.environment, config.environment_name().to_owned()))
            .map_err(|err| err.error_code())
    }

    fn seeder_decides(row: &Row, path: &Path, profile: BuildProfile) -> Outcome {
        guard_environment_from(&vars(row, true), path, profile)
            .map(|resolved| (resolved.environment(), resolved.name().to_owned()))
            .map_err(|err| err.error_code())
    }

    // Only a debug suite runs this, so the release half is asked for through the profile argument.
    #[test]
    fn the_loader_and_the_seeder_agree_on_every_posture_row() {
        for profile in [BuildProfile::Release, BuildProfile::Debug] {
            for row in &ROWS {
                let dir = TempDir::new().expect("tempdir");
                let path = manifest(&dir, row);
                let loader = loader_decides(row, &path, profile);
                let seeder = seeder_decides(row, &path, profile);
                assert_eq!(loader, seeder, "{} in a {profile} build", row.label);
            }
        }
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn the_posture_table_decides_what_it_should() {
        let expected: [(&str, BuildProfile, Outcome); 12] = [
            ("unset", BuildProfile::Release, Err("CFG_019")),
            (
                "unset",
                BuildProfile::Debug,
                Ok((Environment::Development, "development".into())),
            ),
            (
                "built-in",
                BuildProfile::Release,
                Ok((Environment::Production, "production".into())),
            ),
            (
                "built-in",
                BuildProfile::Debug,
                Ok((Environment::Production, "production".into())),
            ),
            (
                "alias",
                BuildProfile::Release,
                Ok((Environment::Production, "production".into())),
            ),
            (
                "alias",
                BuildProfile::Debug,
                Ok((Environment::Production, "production".into())),
            ),
            (
                "declared deployed",
                BuildProfile::Release,
                Ok((Environment::Production, "devops".into())),
            ),
            (
                "declared deployed",
                BuildProfile::Debug,
                Ok((Environment::Production, "devops".into())),
            ),
            (
                "declared development",
                BuildProfile::Release,
                Ok((Environment::Development, "laptop".into())),
            ),
            (
                "declared development",
                BuildProfile::Debug,
                Ok((Environment::Development, "laptop".into())),
            ),
            ("undeclared", BuildProfile::Release, Err("CFG_020")),
            (
                "undeclared",
                BuildProfile::Debug,
                Ok((Environment::Development, "producton".into())),
            ),
        ];
        for (label, profile, outcome) in expected {
            let row = ROWS
                .iter()
                .find(|row| row.label == label)
                .expect("row exists");
            let dir = TempDir::new().expect("tempdir");
            let path = manifest(&dir, row);
            assert_eq!(
                seeder_decides(row, &path, profile),
                outcome,
                "{label} in a {profile} build"
            );
        }
    }

    #[test]
    fn a_release_seeder_refuses_an_unset_or_undeclared_environment() {
        for label in ["unset", "undeclared"] {
            let row = ROWS
                .iter()
                .find(|row| row.label == label)
                .expect("row exists");
            let dir = TempDir::new().expect("tempdir");
            let err = guard_environment_from(
                &vars(row, true),
                manifest(&dir, row),
                BuildProfile::Release,
            )
            .expect_err("the override is for a deployed environment, not a guessed one");
            assert!(matches!(err, SeedError::Config(_)), "{label}: {err:?}");
            assert!(!err.is_retryable());
        }
    }

    #[test]
    fn a_declared_development_environment_seeds_without_the_override() {
        let row = ROWS
            .iter()
            .find(|row| row.label == "declared development")
            .expect("row");
        let dir = TempDir::new().expect("tempdir");
        let resolved = guard_environment_from(
            &vars(row, false),
            manifest(&dir, row),
            BuildProfile::Release,
        )
        .expect("declared development needs no flag");
        assert_eq!(resolved.name(), "laptop");
    }

    #[test]
    fn a_declared_deployed_environment_needs_the_override() {
        let row = ROWS
            .iter()
            .find(|row| row.label == "declared deployed")
            .expect("row");
        let dir = TempDir::new().expect("tempdir");
        let err =
            guard_environment_from(&vars(row, false), manifest(&dir, row), BuildProfile::Debug)
                .expect_err("deployed is guarded in every profile");
        assert!(
            matches!(&err, SeedError::EnvironmentNotAllowed { environment } if environment == "devops")
        );
        assert_eq!(err.error_code(), "SEED_003");
    }
}
