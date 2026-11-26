//! TOML parsers for sections that need more than plain `serde` deserialization.

mod apps;
pub mod database;
mod domains;
mod messaging;

pub(crate) use apps::parse_apps_config;
pub(crate) use database::parse_database_settings;
pub(crate) use domains::parse_domains_config;
pub(crate) use messaging::parse_messaging_settings;
