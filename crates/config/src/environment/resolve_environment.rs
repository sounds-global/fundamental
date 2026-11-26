//! [`resolve_environment`]: the one place a process's environment is decided.

use super::{EnvironmentOrigin, ResolvedEnvironment};
use crate::{ConfigError, ConfigResult};

use fundamental_core::{BuildProfile, Environment, EnvironmentPosture, ParsePostureError};

use toml::Value;
use toml::map::Map;
use tracing::{info, instrument, warn};

/// Decides the environment a process runs as, from its origin, the manifest and the build profile.
///
/// # Errors
/// [`ConfigError::EnvironmentUnset`], [`ConfigError::EnvironmentPostureUndeclared`],
/// [`ConfigError::EnvironmentPostureConflict`], or [`ConfigError::InvalidValue`] for a bad `posture`.
#[instrument(skip(manifest), fields(%origin, %profile), err(Debug))]
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "readability baseline"
)]
pub fn resolve_environment(
    origin: EnvironmentOrigin,
    manifest: Option<&Value>,
    profile: BuildProfile,
) -> ConfigResult<ResolvedEnvironment> {
    let tables = manifest
        .and_then(|manifest| manifest.get("environments"))
        .and_then(Value::as_table);
    if let Some(tables) = tables {
        refuse_undeclarable_tables(tables)?;
    }

    let (environment, name) = match &origin {
        EnvironmentOrigin::Unset => match profile {
            BuildProfile::Release => return Err(ConfigError::EnvironmentUnset),
            BuildProfile::Debug => {
                warn!(
                    env_var = Environment::ENV_VAR,
                    %profile,
                    "FUNDAMENTAL_ENV is unset; a debug build runs as development"
                );
                (Environment::Development, None)
            }
        },
        EnvironmentOrigin::Variable(raw) => match raw.parse::<Environment>() {
            Ok(environment) => (environment, None),
            Err(_) => match declared_posture(tables, raw)? {
                Some(posture) => (posture.enforced_as(), Some(raw.clone())),
                None => match profile {
                    BuildProfile::Release => {
                        return Err(ConfigError::EnvironmentPostureUndeclared {
                            name: raw.clone(),
                        });
                    }
                    BuildProfile::Debug => {
                        warn!(
                            environment = %raw,
                            %profile,
                            "environment declares no posture; a debug build runs it as development"
                        );
                        (Environment::Development, Some(raw.clone()))
                    }
                },
            },
        },
        EnvironmentOrigin::Chosen { environment, name } => {
            if let Some(name) = name.as_deref()
                && let Some(posture) = declared_posture(tables, name)?
                && posture != environment.posture()
            {
                return Err(ConfigError::EnvironmentPostureConflict {
                    table: name.to_owned(),
                    reason: format!(
                        "it declares posture = \"{posture}\", but the code chose {environment}, \
                         whose posture is \"{}\"",
                        environment.posture()
                    ),
                });
            }
            (*environment, name.clone())
        }
    };

    let resolved = ResolvedEnvironment::new(environment, name, origin);
    info!(
        environment = resolved.name(),
        enforced_as = %resolved.environment(),
        posture = %resolved.posture(),
        origin = %resolved.origin(),
        %profile,
        "environment resolved"
    );
    Ok(resolved)
}

/// Only a custom name can have one; [`refuse_undeclarable_tables`] refused the rest.
fn declared_posture(
    tables: Option<&Map<String, Value>>,
    name: &str,
) -> ConfigResult<Option<EnvironmentPosture>> {
    let Some(value) = tables
        .and_then(|tables| tables.get(name))
        .and_then(|table| table.get(EnvironmentPosture::KEY))
    else {
        return Ok(None);
    };
    let field = format!("environments.{name}.{}", EnvironmentPosture::KEY);
    let spelled = value.as_str().ok_or_else(|| {
        ConfigError::invalid_value(&field, format!("must be {}", EnvironmentPosture::ACCEPTED))
    })?;
    spelled
        .parse()
        .map(Some)
        .map_err(|err: ParsePostureError| ConfigError::invalid_value(&field, err.to_string()))
}

fn refuse_undeclarable_tables(tables: &Map<String, Value>) -> ConfigResult<()> {
    for (table, body) in tables {
        let Ok(builtin) = table.parse::<Environment>() else {
            continue;
        };
        if table != builtin.as_str() {
            return Err(ConfigError::EnvironmentPostureConflict {
                table: table.clone(),
                reason: format!(
                    "'{table}' is an alias of {builtin}, and overrides are read only from \
                     [environments.{builtin}]; rename the table"
                ),
            });
        }
        if body.get(EnvironmentPosture::KEY).is_some() {
            return Err(ConfigError::EnvironmentPostureConflict {
                table: table.clone(),
                reason: format!(
                    "the posture of {builtin} is fixed at \"{}\"; remove the posture key",
                    builtin.posture()
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;
    use tracing_test::traced_test;

    const RELEASE: BuildProfile = BuildProfile::Release;
    const DEBUG: BuildProfile = BuildProfile::Debug;

    fn manifest(toml: &str) -> Value {
        toml::from_str(toml).expect("test manifest parses")
    }

    fn variable(name: &str) -> EnvironmentOrigin {
        EnvironmentOrigin::Variable(name.to_owned())
    }

    #[test]
    fn an_unset_environment_refuses_a_release_build() {
        let err = resolve_environment(EnvironmentOrigin::Unset, None, RELEASE)
            .expect_err("a release build never guesses");
        assert!(matches!(err, ConfigError::EnvironmentUnset));
        assert_eq!(err.error_code(), "CFG_019");
        assert!(!err.is_retryable());
        let message = err.to_string();
        for accepted in ["development", "test", "staging", "production", "posture"] {
            assert!(message.contains(accepted), "{message}");
        }
    }

    #[traced_test]
    #[test]
    fn a_debug_build_defaults_to_development_and_warns() {
        let resolved = resolve_environment(EnvironmentOrigin::Unset, None, DEBUG)
            .expect("a debug build runs as development");
        assert_eq!(resolved.environment(), Environment::Development);
        assert_eq!(resolved.custom_name(), None);
        assert!(logs_contain("WARN"));
        assert!(logs_contain("FUNDAMENTAL_ENV is unset"));
        assert!(logs_contain("environment resolved"));
    }

    #[test]
    fn an_undeclared_custom_environment_is_refused() {
        for manifest in [
            None,
            Some(manifest("[environments.devops]\n")),
            Some(manifest(
                "[environments.devops.apps.main]\nweb = { port = 1 }\n",
            )),
        ] {
            let err = resolve_environment(variable("producton"), manifest.as_ref(), RELEASE)
                .expect_err("a typo is not an environment");
            assert!(
                matches!(&err, ConfigError::EnvironmentPostureUndeclared { name } if name == "producton")
            );
            assert_eq!(err.error_code(), "CFG_020");
            assert!(!err.is_retryable());

            let err = resolve_environment(variable("devops"), manifest.as_ref(), RELEASE)
                .expect_err("a table without posture is undeclared");
            assert_eq!(err.error_code(), "CFG_020");
            assert!(err.to_string().contains("[environments.devops]"));
        }
    }

    #[traced_test]
    #[test]
    fn an_undeclared_custom_environment_warns_in_a_debug_build() {
        let resolved = resolve_environment(variable("devops"), None, DEBUG)
            .expect("a debug build runs it as development");
        assert_eq!(resolved.environment(), Environment::Development);
        assert_eq!(resolved.custom_name(), Some("devops"));
        assert!(logs_contain("declares no posture"));
    }

    #[test]
    fn a_declared_posture_is_enforced_in_both_profiles() {
        let declared = manifest(
            "[environments.devops]\nposture = \"deployed\"\n\
             [environments.laptop]\nposture = \"development\"\n",
        );
        for profile in [RELEASE, DEBUG] {
            let devops = resolve_environment(variable("devops"), Some(&declared), profile)
                .expect("declared deployed");
            assert_eq!(devops.environment(), Environment::Production);
            assert_eq!(devops.posture(), EnvironmentPosture::Deployed);
            assert_eq!(devops.name(), "devops");

            let laptop = resolve_environment(variable("laptop"), Some(&declared), profile)
                .expect("declared development");
            assert_eq!(laptop.environment(), Environment::Development);
            assert_eq!(laptop.name(), "laptop");
        }
    }

    #[test]
    fn a_builtin_or_alias_resolves_to_the_builtin_in_both_profiles() {
        for profile in [RELEASE, DEBUG] {
            for (spelled, environment) in [
                ("production", Environment::Production),
                ("prod", Environment::Production),
                ("staging", Environment::Staging),
                ("development", Environment::Development),
                ("ci", Environment::Test),
            ] {
                let resolved = resolve_environment(variable(spelled), None, profile)
                    .expect("a built-in needs no declaration");
                assert_eq!(resolved.environment(), environment);
                assert_eq!(resolved.custom_name(), None);
            }
        }
    }

    #[test]
    fn a_builtin_posture_cannot_be_declared() {
        let downgrade = manifest("[environments.production]\nposture = \"development\"\n");
        let err = resolve_environment(variable("production"), Some(&downgrade), RELEASE)
            .expect_err("production's posture is fixed");
        assert!(
            matches!(&err, ConfigError::EnvironmentPostureConflict { table, .. } if table == "production")
        );
        assert_eq!(err.error_code(), "CFG_021");

        let err = resolve_environment(variable("development"), Some(&downgrade), DEBUG)
            .expect_err("refused from any environment");
        assert_eq!(err.error_code(), "CFG_021");

        let agreeing = manifest("[environments.staging]\nposture = \"deployed\"\n");
        let err = resolve_environment(variable("staging"), Some(&agreeing), RELEASE)
            .expect_err("even an agreeing posture reads as if it could be changed");
        assert_eq!(err.error_code(), "CFG_021");
    }

    #[test]
    fn an_alias_named_table_is_refused() {
        for alias in ["prod", "stage", "dev", "ci", "Production"] {
            let aliased = manifest(&format!("[environments.{alias}]\nposture = \"deployed\"\n"));
            let err = resolve_environment(variable("production"), Some(&aliased), RELEASE)
                .expect_err("an alias table is never read");
            assert!(
                matches!(&err, ConfigError::EnvironmentPostureConflict { table, .. } if table == alias),
                "{alias}: {err:?}"
            );
        }
    }

    #[test]
    fn an_explicit_environment_is_never_loosened_by_the_manifest() {
        let loosening = manifest("[environments.devops]\nposture = \"development\"\n");
        let chosen = EnvironmentOrigin::Chosen {
            environment: Environment::Production,
            name: Some("devops".to_owned()),
        };
        let err = resolve_environment(chosen, Some(&loosening), RELEASE)
            .expect_err("the code chose production");
        assert!(
            matches!(&err, ConfigError::EnvironmentPostureConflict { table, .. } if table == "devops")
        );

        let undeclared = manifest("[environments.devops]\n");
        let chosen = EnvironmentOrigin::Chosen {
            environment: Environment::Production,
            name: Some("devops".to_owned()),
        };
        let resolved = resolve_environment(chosen, Some(&undeclared), RELEASE)
            .expect("nothing to disagree with");
        assert_eq!(resolved.environment(), Environment::Production);
        assert_eq!(resolved.name(), "devops");

        let chosen = EnvironmentOrigin::Chosen {
            environment: Environment::Development,
            name: None,
        };
        let resolved = resolve_environment(chosen, None, RELEASE)
            .expect("an explicit development is explicit");
        assert_eq!(resolved.environment(), Environment::Development);
    }

    #[test]
    fn a_posture_is_spelled_exactly() {
        for bad in ["\"deploy\"", "\"Deployed\"", "true", "\"production\""] {
            let bad = manifest(&format!("[environments.devops]\nposture = {bad}\n"));
            let err = resolve_environment(variable("devops"), Some(&bad), DEBUG)
                .expect_err("a near-miss is refused in every profile");
            assert!(
                matches!(&err, ConfigError::InvalidValue { field, .. } if field == "environments.devops.posture"),
                "{err:?}"
            );
        }
    }

    #[traced_test]
    #[test]
    fn the_resolution_names_environment_posture_and_origin() {
        let declared = manifest("[environments.devops]\nposture = \"deployed\"\n");
        resolve_environment(variable("devops"), Some(&declared), RELEASE).expect("declared");
        assert!(logs_contain("environment resolved"));
        assert!(logs_contain("environment=\"devops\""));
        assert!(logs_contain("enforced_as=production"));
        assert!(logs_contain("posture=deployed"));
        assert!(logs_contain("origin=variable"));
    }
}
