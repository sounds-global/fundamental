//! Parser for the `[apps]` configuration section.

use super::{parse_database_settings, parse_messaging_settings};
use crate::settings::{AppConfig, AppsConfig, CustomSettings, SurfaceEntryConfig};
use crate::{ConfigError, ConfigResult};

use std::collections::HashMap;

/// Parses each app table into its typed config sections and its surface entries.
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
pub(crate) fn parse_apps_config(value: &toml::Value) -> ConfigResult<AppsConfig> {
    let table = value
        .as_table()
        .ok_or_else(|| ConfigError::invalid_value("apps", "expected table"))?;

    let mut config = AppsConfig::default();

    for (app_name, app_value) in table {
        let app_table = app_value.as_table().ok_or_else(|| {
            ConfigError::invalid_value(format!("apps.{app_name}"), "expected table")
        })?;

        let mut app = AppConfig::default();
        let mut surface_table = toml::map::Map::new();

        for (key, value) in app_table {
            match key.as_str() {
                "database" => {
                    app.database = Some(parse_database_settings(value)?);
                }
                "messaging" => {
                    app.messaging = Some(parse_messaging_settings(value)?);
                }
                "auth" => {
                    app.auth = Some(value.clone().try_into().map_err(|e: toml::de::Error| {
                        ConfigError::invalid_value(format!("apps.{app_name}.auth"), e.to_string())
                    })?);
                }
                "authz" => {
                    app.authz = Some(value.clone().try_into().map_err(|e: toml::de::Error| {
                        ConfigError::invalid_value(format!("apps.{app_name}.authz"), e.to_string())
                    })?);
                }
                "client" => {
                    app.client = Some(value.clone().try_into().map_err(|e: toml::de::Error| {
                        ConfigError::invalid_value(format!("apps.{app_name}.client"), e.to_string())
                    })?);
                }
                "observability" => {
                    app.observability =
                        Some(value.clone().try_into().map_err(|e: toml::de::Error| {
                            ConfigError::invalid_value(
                                format!("apps.{app_name}.observability"),
                                e.to_string(),
                            )
                        })?);
                }
                "custom" => {
                    app.custom = Some(CustomSettings::from_toml(value.clone()));
                }
                crate::loader::BRAND => {
                    let brand = value.as_str().ok_or_else(|| {
                        ConfigError::invalid_value(
                            format!("apps.{app_name}.brand"),
                            "expected a brand code",
                        )
                    })?;
                    app.brand = Some(brand.to_owned());
                }
                crate::loader::INSTANCE_IDENTITY => {
                    app.instance_identity = value
                        .as_array()
                        .and_then(|sections| {
                            sections
                                .iter()
                                .map(|section| section.as_str().map(str::to_owned))
                                .collect::<Option<Vec<_>>>()
                        })
                        .ok_or_else(|| {
                            ConfigError::invalid_value(
                                format!("apps.{app_name}.instance_identity"),
                                "expected a list of sections and leaves",
                            )
                        })?;
                }
                // Resolved by the loader for the active app only; for others the key is raw, and no surface.
                crate::loader::INSTANCE_OF => {}
                "surfaces" => {
                    if let Some(arr) = value.as_array() {
                        for s in arr {
                            if let Some(name) = s.as_str() {
                                app.surfaces
                                    .insert(name.to_string(), SurfaceEntryConfig::default());
                            }
                        }
                    }
                }
                _ => {
                    surface_table.insert(key.clone(), value.clone());
                }
            }
        }

        let surfaces_value = toml::Value::Table(surface_table);
        let parsed_surfaces: HashMap<String, SurfaceEntryConfig> =
            surfaces_value.try_into().map_err(|e: toml::de::Error| {
                ConfigError::invalid_value(format!("apps.{app_name}"), e.to_string())
            })?;
        app.surfaces.extend(parsed_surfaces);

        config.apps.insert(app_name.clone(), app);
    }

    Ok(config)
}
