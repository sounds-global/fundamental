//! Database configuration settings ([`DatabaseSettings`]).

mod connection_ref;
mod cql;
mod credentials;
mod named;
mod sql;

pub use connection_ref::ConnectionRef;
pub use cql::CqlSettings;
pub use credentials::CredentialSource;
pub use named::NamedDatabaseSettings;
pub use sql::SqlSettings;

use serde::{Deserialize, Serialize};

/// Database configuration settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseSettings {
    /// YSQL configuration (default pool).
    pub sql: Option<SqlSettings>,
    /// YCQL configuration (default pool).
    pub cql: Option<CqlSettings>,
    /// Named database connections for multi-domain routing (e.g. `[database.sql.partner]`).
    pub named: Option<NamedDatabaseSettings>,
}

impl DatabaseSettings {
    /// Every instance this configuration will open: both engines' default pools and every named one.
    pub fn connections(&self) -> impl Iterator<Item = ConnectionRef<'_>> {
        let default_sql = self.sql.iter().map(|c| ConnectionRef {
            engine: SQL,
            name: None,
            credentials: &c.credentials,
            tls: c.tls,
            max_connections: c.max_connections,
        });
        let default_cql = self.cql.iter().map(|s| ConnectionRef {
            engine: CQL,
            name: None,
            credentials: &s.credentials,
            tls: s.tls,
            max_connections: cql_connections(s),
        });
        let named = self.named.iter().flat_map(|named| {
            let sql = named.sql.iter().map(|(name, c)| ConnectionRef {
                engine: SQL,
                name: Some(name.as_str()),
                credentials: &c.credentials,
                tls: c.tls,
                max_connections: c.max_connections,
            });
            let cql = named.cql.iter().map(|(name, s)| ConnectionRef {
                engine: CQL,
                name: Some(name.as_str()),
                credentials: &s.credentials,
                tls: s.tls,
                max_connections: cql_connections(s),
            });
            sql.chain(cql)
        });

        default_sql.chain(default_cql).chain(named)
    }

    /// Whether any configured instance sources its credentials from a secret manager.
    #[must_use]
    pub fn requires_secret_manager(&self) -> bool {
        self.connections()
            .any(|c| c.credentials.requires_secret_manager())
    }
}

const SQL: &str = "sql";

const CQL: &str = "cql";

/// The driver keeps a pool per node, so the cluster sees the product, saturating.
fn cql_connections(settings: &CqlSettings) -> u32 {
    let hosts = u32::try_from(settings.hosts.len()).unwrap_or(u32::MAX);
    settings.connections_per_host.saturating_mul(hosts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn named_sql(name: &str) -> DatabaseSettings {
        let mut sql = HashMap::new();
        sql.insert(name.to_string(), SqlSettings::default());
        DatabaseSettings {
            sql: None,
            cql: None,
            named: Some(NamedDatabaseSettings {
                sql,
                cql: HashMap::new(),
            }),
        }
    }

    #[test]
    fn a_named_only_configuration_still_enumerates_its_instance() {
        let settings = named_sql("layer0");
        let connections: Vec<_> = settings.connections().collect();

        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].label(), "sql.layer0");
    }

    #[test]
    fn a_default_pool_is_labelled_by_its_engine_alone() {
        let settings = DatabaseSettings {
            sql: Some(SqlSettings::default()),
            cql: Some(CqlSettings::default()),
            named: None,
        };
        let labels: Vec<_> = settings.connections().map(|c| c.label()).collect();

        assert_eq!(labels, vec!["sql", "cql"]);
    }

    #[test]
    fn a_sql_connection_carries_its_pool_size() {
        let sql = SqlSettings {
            max_connections: 25,
            ..SqlSettings::default()
        };
        let settings = DatabaseSettings {
            sql: Some(sql),
            cql: None,
            named: None,
        };

        let connections: Vec<_> = settings.connections().collect();
        assert_eq!(connections[0].max_connections, 25);
    }

    #[test]
    fn a_cql_connection_reports_the_whole_ring_not_one_host() {
        let cql = CqlSettings {
            hosts: vec![
                "yb-1:9042".to_string(),
                "yb-2:9042".to_string(),
                "yb-3:9042".to_string(),
            ],
            connections_per_host: 4,
            ..CqlSettings::default()
        };
        let settings = DatabaseSettings {
            sql: None,
            cql: Some(cql),
            named: None,
        };

        let connections: Vec<_> = settings.connections().collect();
        assert_eq!(connections[0].max_connections, 12);
    }

    #[test]
    fn both_engines_and_both_slots_are_enumerated() {
        let mut settings = named_sql("layer0");
        settings.cql = Some(CqlSettings::default());
        if let Some(ref mut named) = settings.named {
            named
                .cql
                .insert("layer4".to_string(), CqlSettings::default());
        }

        let mut labels: Vec<_> = settings.connections().map(|c| c.label()).collect();
        labels.sort();

        // Written out rather than compared as a set, because the sort is under test.
        assert_eq!(labels, vec!["cql", "cql.layer4", "sql.layer0"]);
    }
}
