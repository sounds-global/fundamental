//! Parser for the `[domains]` configuration section.

use super::parse_database_settings;
use crate::settings::{DatabaseSettings, DomainConfig, DomainsConfig};
use crate::{ConfigError, ConfigResult};

pub(crate) fn parse_domains_config(value: &toml::Value) -> ConfigResult<DomainsConfig> {
    let table = value
        .as_table()
        .ok_or_else(|| ConfigError::invalid_value("domains", "expected table"))?;

    let mut config = DomainsConfig::default();

    for (domain_name, domain_value) in table {
        let domain_table = domain_value.as_table().ok_or_else(|| {
            ConfigError::invalid_value(format!("domains.{domain_name}"), "expected table")
        })?;

        let feature = domain_table
            .get("feature")
            .and_then(|v| v.as_str())
            .unwrap_or(domain_name.as_str())
            .to_string();

        let database = if let Some(db_value) = domain_table.get("database") {
            parse_database_settings(db_value)?
        } else {
            DatabaseSettings::default()
        };

        config
            .domains
            .insert(domain_name.clone(), DomainConfig { feature, database });
    }

    Ok(config)
}
