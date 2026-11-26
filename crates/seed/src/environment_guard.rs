use fundamental_config::ResolvedEnvironment;
use fundamental_core::EnvironmentPosture;
use tracing::{info, instrument, warn};

use crate::SeedError;

/// The environment variable that opts a seeder into a deployed environment: non-empty and not `0`.
pub const RELEASE_OVERRIDE_VAR: &str = "FUNDAMENTAL_SEED_RELEASE";

/// Checks whether seeding is allowed in `environment`: always under a development posture,
/// under a deployed one only with `allow_non_dev`.
///
/// # Errors
///
/// Returns [`SeedError::EnvironmentNotAllowed`] when `environment` is deployed and
/// `allow_non_dev` is `false`.
pub fn guard_environment(
    environment: &ResolvedEnvironment,
    allow_non_dev: bool,
) -> Result<(), SeedError> {
    guard_posture(environment.posture(), environment.name(), allow_non_dev)
}

#[instrument(skip_all, fields(environment = name, %posture, allow_non_dev))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn guard_posture(
    posture: EnvironmentPosture,
    name: &str,
    allow_non_dev: bool,
) -> Result<(), SeedError> {
    if posture == EnvironmentPosture::Development {
        info!("environment permits seeding");
        return Ok(());
    }

    if allow_non_dev {
        warn!("seeding a deployed environment via the explicit release override");
        return Ok(());
    }

    warn!("refusing to seed a deployed environment without the release override");
    Err(SeedError::EnvironmentNotAllowed {
        environment: name.to_owned(),
    })
}

/// Whether a [`RELEASE_OVERRIDE_VAR`] value enables the override.
pub(crate) fn override_enabled(value: Option<&str>) -> bool {
    matches!(value, Some(v) if !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_config::{EnvironmentOrigin, resolve_environment};
    use fundamental_core::{BuildProfile, Environment, Error as _};

    fn resolved(environment: Environment, name: Option<&str>) -> ResolvedEnvironment {
        let origin = EnvironmentOrigin::Chosen {
            environment,
            name: name.map(str::to_owned),
        };
        resolve_environment(origin, None, BuildProfile::Release).expect("chosen resolves")
    }

    #[test]
    fn local_environments_are_always_allowed() {
        for environment in [Environment::Development, Environment::Test] {
            assert!(guard_environment(&resolved(environment, None), false).is_ok());
        }
    }

    #[test]
    fn deployed_environments_are_refused_by_default() {
        for environment in [Environment::Staging, Environment::Production] {
            let err = guard_environment(&resolved(environment, None), false)
                .expect_err("deployed must be refused");
            assert!(matches!(err, SeedError::EnvironmentNotAllowed { .. }));
            assert_eq!(err.error_code(), "SEED_003");
            assert!(err.to_string().contains(environment.as_str()));
        }
    }

    #[test]
    fn a_refusal_names_the_custom_environment_not_the_builtin_it_enforces() {
        let err = guard_environment(&resolved(Environment::Production, Some("devops")), false)
            .expect_err("deployed must be refused");
        assert!(
            matches!(&err, SeedError::EnvironmentNotAllowed { environment } if environment == "devops")
        );
    }

    #[test]
    fn release_override_permits_deployed_environments() {
        for environment in [Environment::Staging, Environment::Production] {
            assert!(guard_environment(&resolved(environment, None), true).is_ok());
        }
    }

    #[test]
    fn override_is_enabled_only_for_a_meaningful_value() {
        assert!(!override_enabled(None));
        assert!(!override_enabled(Some("")));
        assert!(!override_enabled(Some("0")));
        assert!(override_enabled(Some("1")));
        assert!(override_enabled(Some("true")));
    }
}
