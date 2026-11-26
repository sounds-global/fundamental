//! The project's settings as a config source ([`FundamentalTomlSource`]).

use super::{ConfigResult, ConfigSource};
use crate::ConfigError;
use fundamental_core::EnvironmentPosture;

/// The project's settings as read for the active environment, the same read it was resolved from.
pub struct FundamentalTomlSource {
    /// The settings, `fun.toml` with the active environment's override laid over it.
    pub manifest: toml::Value,
}

impl ConfigSource for FundamentalTomlSource {
    fn name(&self) -> &str {
        "fun.toml"
    }

    fn load(&self) -> ConfigResult<Option<toml::Value>> {
        let mut result = self.manifest.clone();
        if let toml::Value::Table(ref mut table) = result
            && let Some(toml::Value::Table(environments)) = table.remove("environments")
        {
            refuse_settings(&environments)?;
        }
        Ok(Some(result))
    }
}

/// Refuses an `[environments.<name>]` table holding anything but its posture.
fn refuse_settings(environments: &toml::Table) -> ConfigResult<()> {
    for (name, declared) in environments {
        let holds_settings = declared
            .as_table()
            .is_some_and(|table| table.keys().any(|key| key != EnvironmentPosture::KEY));
        if holds_settings {
            return Err(ConfigError::EnvironmentOverridesInManifest { name: name.clone() });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn a_posture_is_declared_and_never_a_setting() {
        let manifest: toml::Value = toml::from_str(
            "[apps.main]\nweb = { port = 1 }\n[environments.devops]\nposture = \"deployed\"\n",
        )
        .expect("manifest parses");
        let merged = FundamentalTomlSource { manifest }
            .load()
            .expect("loads")
            .expect("has content");
        assert!(merged.get(EnvironmentPosture::KEY).is_none(), "{merged}");
        assert!(merged.get("environments").is_none());
        assert_eq!(merged["apps"]["main"]["web"]["port"].as_integer(), Some(1));
    }

    #[test]
    fn an_environment_table_holding_settings_is_refused() {
        let manifest: toml::Value =
            toml::from_str("[environments.devops.apps.main]\nweb = { port = 2 }\n")
                .expect("manifest parses");
        let refused = FundamentalTomlSource { manifest }.load();
        assert!(
            matches!(&refused, Err(error) if error.error_code() == "CFG_033"),
            "{refused:?}"
        );
    }
}
