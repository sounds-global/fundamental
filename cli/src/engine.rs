//! The [`Engine`] of a database migration directory and its file extension.

/// A database engine a migration set targets; an unknown name is kept as [`Other`](Engine::Other).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Engine {
    /// YugabyteDB YSQL (PostgreSQL-compatible SQL).
    SqlDb,
    /// YugabyteDB YCQL (Cassandra-compatible CQL).
    CqlDb,
    /// Any other engine name, kept verbatim as the directory name.
    Other(String),
}

impl Engine {
    /// Parse an engine name, case-insensitively for the known engines.
    pub(crate) fn parse(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "sql" => Engine::SqlDb,
            "cql" => Engine::CqlDb,
            _ => Engine::Other(name.to_string()),
        }
    }

    /// The `migrations/<dir>/` subdirectory name for this engine.
    pub(crate) fn dir_name(&self) -> &str {
        match self {
            Engine::SqlDb => "sql",
            Engine::CqlDb => "cql",
            Engine::Other(name) => name,
        }
    }

    /// The migration-script file extension (`sql` or `cql`).
    pub(crate) fn extension(&self) -> &'static str {
        match self {
            Engine::CqlDb => "cql",
            Engine::SqlDb | Engine::Other(_) => "sql",
        }
    }

    /// The `fundamental` cargo feature that enables this engine's backend; unknown engines get `sql`.
    pub(crate) fn feature(&self) -> &'static str {
        match self {
            Engine::CqlDb => "cql",
            Engine::SqlDb | Engine::Other(_) => "sql",
        }
    }

    /// The `[apps.<app>.database.<key>]` table key for this engine.
    pub(crate) fn config_key(&self) -> &'static str {
        self.feature()
    }

    /// The default `[apps.<app>.database.<key>]` fields of a scaffolded app, shared by `fun new` and `fun add app`.
    pub(crate) fn scaffold_config(&self, db: &str) -> Vec<(&'static str, DbConfigValue)> {
        match self {
            Engine::CqlDb => vec![
                (
                    "hosts",
                    DbConfigValue::List(vec!["127.0.0.1:9042".to_string()]),
                ),
                ("keyspace", DbConfigValue::Str(db.to_string())),
            ],
            Engine::SqlDb | Engine::Other(_) => vec![(
                "url",
                DbConfigValue::Str(format!(
                    "postgresql://yugabyte@localhost:5433/{db}?sslmode=disable"
                )),
            )],
        }
    }
}

/// A default value for a database config field: a string or a list of strings.
pub(crate) enum DbConfigValue {
    Str(String),
    List(Vec<String>),
}

impl DbConfigValue {
    /// Render this value as a `key = value` TOML line, with a trailing newline.
    pub(crate) fn render_toml(&self, key: &str) -> String {
        match self {
            DbConfigValue::Str(s) => format!("{key} = \"{s}\"\n"),
            DbConfigValue::List(items) => {
                let joined = items
                    .iter()
                    .map(|i| format!("\"{i}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{key} = [{joined}]\n")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_engines_map_to_extensions() {
        assert_eq!(Engine::parse("sql").extension(), "sql");
        assert_eq!(Engine::parse("cql").extension(), "cql");
    }

    #[test]
    fn parsing_is_case_insensitive() {
        assert_eq!(Engine::parse("SQL"), Engine::SqlDb);
        assert_eq!(Engine::parse("Cql"), Engine::CqlDb);
    }

    #[test]
    fn unknown_engine_keeps_its_name_and_defaults_to_sql() {
        let e = Engine::parse("duckdb");
        assert_eq!(e.dir_name(), "duckdb");
        assert_eq!(e.extension(), "sql");
        assert_eq!(e, Engine::Other("duckdb".to_string()));
    }

    #[test]
    fn config_key_collapses_the_sql_family_to_sql() {
        assert_eq!(Engine::SqlDb.config_key(), "sql");
        assert_eq!(Engine::CqlDb.config_key(), "cql");
        assert_eq!(Engine::Other("duckdb".to_string()).config_key(), "sql");
    }

    #[test]
    fn scaffold_config_renders_the_sql_url() {
        let fields = Engine::SqlDb.scaffold_config("billing");
        assert_eq!(fields.len(), 1);
        let (key, value) = &fields[0];
        assert_eq!(*key, "url");
        assert_eq!(
            value.render_toml(key),
            "url = \"postgresql://yugabyte@localhost:5433/billing?sslmode=disable\"\n"
        );
    }

    #[test]
    fn scaffold_config_renders_cql_hosts_and_keyspace() {
        let fields = Engine::CqlDb.scaffold_config("billing");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].0, "hosts");
        assert_eq!(
            fields[0].1.render_toml("hosts"),
            "hosts = [\"127.0.0.1:9042\"]\n"
        );
        assert_eq!(fields[1].0, "keyspace");
        assert_eq!(
            fields[1].1.render_toml("keyspace"),
            "keyspace = \"billing\"\n"
        );
    }
}
