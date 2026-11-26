//! Parser for the `[database]` section: each engine key's default pool and its named pools.

use crate::settings::{CqlSettings, DatabaseSettings, NamedDatabaseSettings, SqlSettings};
use crate::{ConfigError, ConfigResult};

use fundamental_core::secrets::DatabaseUrl;

/// The default pool's own keys on `[database.sql]`; any other key is a named pool.
pub const SQL_RESERVED: &[&str] = &[
    "url",
    "credentials",
    "max_connections",
    "min_connections",
    "tls",
    "tls_ca_cert_path",
    "tls_ca_cert_pem",
    "tls_client_cert_path",
    "tls_client_cert_pem",
    "tls_client_key_path",
    "tls_client_key_pem",
    "acquire_timeout_secs",
    "statement_timeout_secs",
    "idle_timeout_secs",
    "schema",
    "tls_enabled",
    "connect_timeout_secs",
];

/// Still accepted, so a live `connect_timeout_secs` is never read as a pool of that name.
#[cfg(test)]
const SQL_SUPERSEDED: &[&str] = &["connect_timeout_secs"];

/// The default pool's own keys on `[database.cql]`; any other key is a named pool.
pub const CQL_RESERVED: &[&str] = &[
    "hosts",
    "keyspace",
    "tls",
    "tls_ca_cert_path",
    "tls_ca_cert_pem",
    "tls_client_cert_path",
    "tls_client_cert_pem",
    "tls_client_key_path",
    "tls_client_key_pem",
    "tls_server_name",
    "address_translation",
    "username",
    "password",
    "credentials",
    "connections_per_host",
    "token_aware_routing",
    "tls_enabled",
];

/// Still reserved, so a live `tls_enabled` is never read as a pool of that name.
#[cfg(test)]
const LEGACY_TLS_ALIASES: &[&str] = &["tls_enabled"];

fn resolve_sql_url(settings: &mut SqlSettings) {
    if let Some(url_template) = &settings.url_template {
        settings.url = DatabaseUrl::new(url_template.clone());
    }
}

fn split_pools<'a>(
    engine: &'a toml::Table,
    reserved: &[&str],
) -> (toml::value::Table, Vec<(&'a String, &'a toml::Value)>) {
    let mut default_fields = toml::value::Table::new();
    let mut named = Vec::new();
    for (key, value) in engine {
        if reserved.contains(&key.as_str()) {
            default_fields.insert(key.clone(), value.clone());
        } else {
            named.push((key, value));
        }
    }
    (default_fields, named)
}

fn parse_named_pool<T>(engine: &str, name: &str, value: &toml::Value) -> ConfigResult<T>
where
    T: serde::de::DeserializeOwned,
{
    if !value.is_table() {
        return Err(ConfigError::invalid_value(
            format!("database.{engine}.{name}"),
            "expected a table (named database pool)",
        ));
    }
    value.clone().try_into().map_err(|e: toml::de::Error| {
        ConfigError::invalid_value(format!("database.{engine}.{name}"), e.to_string())
    })
}

/// Parses the default pools and the named pools nested under each engine key.
pub(crate) fn parse_database_settings(value: &toml::Value) -> ConfigResult<DatabaseSettings> {
    let table = value
        .as_table()
        .ok_or_else(|| ConfigError::invalid_value("database", "expected table"))?;

    let mut settings = DatabaseSettings::default();

    if let Some(sql) = table.get("sql") {
        let engine = sql
            .as_table()
            .ok_or_else(|| ConfigError::invalid_value("database.sql", "expected table"))?;
        let (default_fields, named) = split_pools(engine, SQL_RESERVED);

        if !default_fields.is_empty() {
            let mut default: SqlSettings =
                toml::Value::Table(default_fields)
                    .try_into()
                    .map_err(|e: toml::de::Error| {
                        ConfigError::invalid_value("database.sql", e.to_string())
                    })?;
            resolve_sql_url(&mut default);
            settings.sql = Some(default);
        }

        for (name, value) in named {
            let mut pool: SqlSettings = parse_named_pool("sql", name, value)?;
            resolve_sql_url(&mut pool);
            settings
                .named
                .get_or_insert_with(NamedDatabaseSettings::default)
                .sql
                .insert(name.clone(), pool);
        }
    }

    if let Some(cql) = table.get("cql") {
        let engine = cql
            .as_table()
            .ok_or_else(|| ConfigError::invalid_value("database.cql", "expected table"))?;
        let (default_fields, named) = split_pools(engine, CQL_RESERVED);

        if !default_fields.is_empty() {
            let default: CqlSettings =
                toml::Value::Table(default_fields)
                    .try_into()
                    .map_err(|e: toml::de::Error| {
                        ConfigError::invalid_value("database.cql", e.to_string())
                    })?;
            settings.cql = Some(default);
        }

        for (name, value) in named {
            let pool: CqlSettings = parse_named_pool("cql", name, value)?;
            settings
                .named
                .get_or_insert_with(NamedDatabaseSettings::default)
                .cql
                .insert(name.clone(), pool);
        }
    }

    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{CredentialSource, TlsPosture};
    use fundamental_core::error::Error as _;
    use fundamental_core::secrets::InMemorySecretString;

    #[test]
    fn parses_default_and_named_pools_with_resolved_urls() {
        let toml = r#"
            [sql]
            url = "postgresql://localhost:5433/app"

            [sql.partner]
            url = "postgresql://localhost:5433/partner"
            max_connections = 5

            [cql.telemetry]
            hosts = ["localhost:9042"]
            keyspace = "telemetry"
        "#;
        let value: toml::Value = toml::from_str(toml).unwrap();
        let settings = parse_database_settings(&value).unwrap();

        let default = settings.sql.as_ref().unwrap();
        assert_eq!(default.url.expose(), "postgresql://localhost:5433/app");

        let named = settings.named.as_ref().expect("named pools parsed");
        let partner = named.sql.get("partner").expect("partner pool");
        assert_eq!(partner.max_connections, 5);
        assert_eq!(partner.url.expose(), "postgresql://localhost:5433/partner");

        let telemetry = named.cql.get("telemetry").expect("telemetry pool");
        assert_eq!(telemetry.keyspace, "telemetry");
    }

    #[test]
    fn a_configured_cql_password_reaches_the_settings() {
        let toml = r#"
            [cql]
            hosts = ["localhost:9042"]
            keyspace = "app"
            username = "app"
            password = "from-a-secret-file"

            [cql.telemetry]
            hosts = ["localhost:9042"]
            keyspace = "telemetry"
            username = "telemetry"
            password = "another-one"
        "#;
        let value: toml::Value = toml::from_str(toml).unwrap();
        let settings = parse_database_settings(&value).unwrap();

        let default = settings.cql.as_ref().expect("default cql pool");
        assert_eq!(default.username.as_deref(), Some("app"));
        assert_eq!(
            default.password.expose().map(String::as_str),
            Some("from-a-secret-file")
        );

        let named = settings.named.as_ref().expect("named pools parsed");
        assert!(
            !named.cql.contains_key("password"),
            "`password` was read as a named pool, which discards the credential: {:?}",
            named.cql.keys().collect::<Vec<_>>()
        );

        let telemetry = named.cql.get("telemetry").expect("telemetry pool");
        assert_eq!(
            telemetry.password.expose().map(String::as_str),
            Some("another-one")
        );
    }

    #[test]
    fn credentials_subtable_is_a_default_field_not_a_named_pool() {
        let toml = r#"
            [sql]
            url = "postgresql://${SQL_HOST}/app"

            [sql.credentials]
            source = "vault_dynamic"
            mount = "database"
            role = "app-sql"
        "#;
        let value: toml::Value = toml::from_str(toml).unwrap();
        let settings = parse_database_settings(&value).unwrap();

        let default = settings.sql.as_ref().expect("default pool");
        assert!(matches!(
            default.credentials,
            CredentialSource::VaultDynamic { .. }
        ));
        assert!(
            settings.named.is_none(),
            "credentials must not be read as a named pool"
        );
    }

    #[test]
    fn a_cql_pool_reads_its_routing_setting() {
        let toml = r#"
            [cql]
            keyspace = "app"
            token_aware_routing = false

            [cql.records]
            keyspace = "records"
            token_aware_routing = false
        "#;
        let value: toml::Value = toml::from_str(toml).unwrap();
        let settings = parse_database_settings(&value).unwrap();

        assert!(
            !settings
                .cql
                .as_ref()
                .expect("default pool")
                .token_aware_routing
        );
        let named = settings.named.as_ref().expect("named pools parsed");
        assert!(!named.cql["records"].token_aware_routing);
    }

    #[test]
    fn named_pool_without_default_leaves_default_none() {
        let toml = r#"
            [sql.partner]
            url = "postgresql://localhost:5433/partner"
        "#;
        let value: toml::Value = toml::from_str(toml).unwrap();
        let settings = parse_database_settings(&value).unwrap();

        assert!(
            settings.sql.is_none(),
            "an engine header with only sub-tables yields no default pool"
        );
        let named = settings.named.as_ref().expect("named pools parsed");
        assert!(named.sql.contains_key("partner"));
    }

    #[test]
    fn scalar_under_engine_key_is_invalid_value() {
        let toml = r#"
            [sql]
            url = "postgresql://localhost:5433/app"
            partner = "oops"
        "#;
        let value: toml::Value = toml::from_str(toml).unwrap();
        let err = parse_database_settings(&value).expect_err("scalar named pool must error");

        assert!(matches!(
            err,
            ConfigError::InvalidValue { ref field, .. } if field == "database.sql.partner"
        ));
        assert_eq!(err.error_code(), "CFG_004");
    }

    #[test]
    fn reserved_sql_keys_match_settings() {
        // A new struct field fails to compile here (no `..Default::default()`), forcing the reserved set along.
        let settings = SqlSettings {
            url: DatabaseUrl::default(),
            url_template: Some("postgresql://localhost/app".to_string()),
            credentials: CredentialSource::Static,
            max_connections: 10,
            min_connections: 2,
            tls: TlsPosture::Enabled,
            tls_ca_cert_path: Some("/etc/ca.pem".to_string()),
            tls_ca_cert_pem: Some("-----BEGIN CERTIFICATE-----".to_string()),
            tls_client_cert_path: Some("/etc/client.pem".to_string()),
            tls_client_cert_pem: Some("-----BEGIN CERTIFICATE-----".to_string()),
            tls_client_key_path: Some("/etc/client-key.pem".to_string()),
            tls_client_key_pem: Some("-----BEGIN PRIVATE KEY-----".to_string()),
            acquire_timeout_secs: Some(2),
            statement_timeout_secs: Some(5),
            idle_timeout_secs: 600,
            schema: Some("tenant_7".to_string()),
        };
        assert_reserved_matches(&settings, SQL_RESERVED, SQL_SUPERSEDED);
    }

    #[test]
    fn reserved_cql_keys_match_settings() {
        let settings = CqlSettings {
            hosts: vec!["localhost:9042".to_string()],
            keyspace: "app".to_string(),
            tls: TlsPosture::Enabled,
            tls_ca_cert_path: Some("/etc/ssl/certs/cluster-ca.pem".to_string()),
            tls_ca_cert_pem: Some("-----BEGIN CERTIFICATE-----".to_string()),
            tls_client_cert_path: Some("/etc/ssl/certs/client.pem".to_string()),
            tls_client_cert_pem: Some("-----BEGIN CERTIFICATE-----".to_string()),
            tls_client_key_path: Some("/etc/ssl/private/client-key.pem".to_string()),
            tls_client_key_pem: Some("-----BEGIN PRIVATE KEY-----".to_string()),
            tls_server_name: Some("cql".to_string()),
            address_translation: vec!["10.0.0.7:9142=127.0.0.1:9142".to_string()],
            username: Some("app".to_string()),
            // Present: an absent `InMemorySecretString` serializes to nothing and would hide the key.
            password: InMemorySecretString::new("s3cret".to_string()),
            credentials: CredentialSource::Static,
            connections_per_host: 1,
            token_aware_routing: false,
        };
        assert_reserved_matches(&settings, CQL_RESERVED, &[]);
    }

    fn assert_reserved_matches<T: serde::Serialize>(
        settings: &T,
        reserved: &[&str],
        superseded: &[&str],
    ) {
        let value = toml::Value::try_from(settings).expect("settings serialize");
        let emitted: std::collections::BTreeSet<&str> = value
            .as_table()
            .expect("settings serialize to a table")
            .keys()
            .map(String::as_str)
            .collect();
        let expected: std::collections::BTreeSet<&str> = emitted
            .iter()
            .copied()
            .chain(LEGACY_TLS_ALIASES.iter().copied())
            .chain(superseded.iter().copied())
            .collect();
        let reserved_set: std::collections::BTreeSet<&str> = reserved.iter().copied().collect();
        assert_eq!(
            reserved_set, expected,
            "reserved-key set is out of sync with the settings struct"
        );
    }
}
