use crate::environment_guard::{guard_posture, override_enabled};
use crate::{RELEASE_OVERRIDE_VAR, SeedError};

use fundamental_config::FundamentalConfig;
use fundamental_core::env::EnvVars;

/// Guards the environment `config` was loaded under, honouring the release override in `vars`.
///
/// # Errors
///
/// Returns [`SeedError::EnvironmentNotAllowed`] when `config` has a deployed posture and
/// the override is not set.
pub fn guard_loaded_config(config: &FundamentalConfig, vars: &EnvVars) -> Result<(), SeedError> {
    let allow = override_enabled(vars.get(RELEASE_OVERRIDE_VAR).as_deref());
    guard_posture(
        config.environment.posture(),
        config.environment_name(),
        allow,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_config::ConfigLoader;
    use fundamental_core::{Environment, Error as _};

    use tempfile::TempDir;

    #[test]
    fn the_guard_judges_the_configuration_the_seeds_use() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("elsewhere")).expect("project directory");
        let manifest = dir.path().join("elsewhere").join("fun.toml");
        std::fs::write(
            &manifest,
            "[environments.devops]\nposture = \"deployed\"\n[apps.main]\nweb = { port = 1 }\n",
        )
        .expect("write manifest");
        let config = ConfigLoader::auto_from(EnvVars::fixed([(Environment::ENV_VAR, "devops")]))
            .with_fundamental_toml(&manifest)
            .load()
            .expect("declared deployed loads");

        let err = guard_loaded_config(&config, &EnvVars::none())
            .expect_err("deployed without the override");
        assert!(
            matches!(&err, SeedError::EnvironmentNotAllowed { environment } if environment == "devops")
        );
        assert_eq!(err.error_code(), "SEED_003");

        let overridden = EnvVars::fixed([(RELEASE_OVERRIDE_VAR, "1")]);
        assert!(guard_loaded_config(&config, &overridden).is_ok());
    }

    #[test]
    fn a_chosen_environment_is_guarded_as_chosen() {
        let dir = TempDir::new().expect("tempdir");
        let manifest = dir.path().join("fun.toml");
        std::fs::write(&manifest, "[apps.main]\nweb = { port = 1 }\n").expect("write manifest");
        let config = ConfigLoader::new(Environment::Production)
            .with_fundamental_toml(&manifest)
            .load()
            .expect("loads");

        let err = guard_loaded_config(&config, &EnvVars::none())
            .expect_err("production chosen in code is deployed");
        assert_eq!(err.error_code(), "SEED_003");

        let config = ConfigLoader::new(Environment::Development)
            .with_fundamental_toml(&manifest)
            .load()
            .expect("loads");
        assert!(guard_loaded_config(&config, &EnvVars::none()).is_ok());
    }
}
