//! Folds app surface overrides into typed settings, and deep-merges TOML.

mod api_settings;
mod server_settings;
mod surface_entry_config;
mod toml;

pub(crate) use api_settings::merge_api_settings;
pub(crate) use server_settings::merge_server_settings;
pub(crate) use surface_entry_config::merge_surface_entry_config;
pub(crate) use toml::merge_toml;
