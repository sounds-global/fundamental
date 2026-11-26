//! Environment variable config source ([`EnvVarSource`]).

use super::{ConfigResult, ConfigSource};
use fundamental_core::env::EnvVars;
use std::collections::HashMap;
use tracing::debug;

/// Loads configuration from prefixed environment variables, a double underscore per nesting level.
///
/// - `FUNDAMENTAL_SERVER__PORT=8080` -> `server.port = 8080`
/// - `FUNDAMENTAL_DATABASE__SQL__MAX_CONNECTIONS=20` -> `database.sql.max_connections = 20`
#[derive(Debug)]
pub struct EnvVarSource {
    prefix: String,
    vars: EnvVars,
}

impl EnvVarSource {
    /// Creates a new environment variable source with the given prefix.
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            vars: EnvVars::process(),
        }
    }

    /// The same source, reading `vars` instead of the process environment.
    #[must_use]
    pub fn with_vars(mut self, vars: EnvVars) -> Self {
        self.vars = vars;
        self
    }

    /// Creates a source with the default "FUNDAMENTAL_" prefix.
    pub fn fundamental() -> Self {
        Self::new("FUNDAMENTAL_")
    }
}

impl ConfigSource for EnvVarSource {
    fn name(&self) -> &str {
        "env-vars"
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn load(&self) -> ConfigResult<Option<toml::Value>> {
        let mut values: HashMap<String, String> = HashMap::new();

        for (key, value) in self.vars.entries() {
            if let Some(suffix) = key.strip_prefix(&self.prefix) {
                let config_key = suffix
                    .to_lowercase()
                    .split("__")
                    .filter(|segment| !segment.is_empty())
                    .collect::<Vec<_>>()
                    .join(".");
                if config_key.is_empty() {
                    continue;
                }
                values.insert(config_key, value);
            }
        }

        if values.is_empty() {
            return Ok(None);
        }

        let mut root = toml::map::Map::new();

        for (key, value) in values {
            insert_dotted(&mut root, &key, parse_value(&value));
        }

        debug!(count = root.len(), prefix = %self.prefix, "loaded env vars");
        Ok(Some(toml::Value::Table(root)))
    }
}

fn insert_dotted(table: &mut toml::map::Map<String, toml::Value>, key: &str, value: toml::Value) {
    let parts: Vec<&str> = key.splitn(2, '.').collect();

    match parts.as_slice() {
        [single] => {
            table.insert((*single).to_string(), value);
        }
        [first, rest] => {
            let entry = table
                .entry((*first).to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));

            if let toml::Value::Table(nested) = entry {
                insert_dotted(nested, rest, value);
            }
        }
        _ => {}
    }
}

fn parse_value(s: &str) -> toml::Value {
    if let Ok(b) = s.parse::<bool>() {
        return toml::Value::Boolean(b);
    }
    if let Ok(i) = s.parse::<i64>() {
        return toml::Value::Integer(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        // A float only with a decimal point.
        if s.contains('.') {
            return toml::Value::Float(f);
        }
    }
    toml::Value::String(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_value() {
        assert_eq!(parse_value("true"), toml::Value::Boolean(true));
        assert_eq!(parse_value("false"), toml::Value::Boolean(false));
        assert_eq!(parse_value("42"), toml::Value::Integer(42));
        assert_eq!(parse_value("1.25"), toml::Value::Float(1.25));
        assert_eq!(
            parse_value("hello"),
            toml::Value::String("hello".to_string())
        );
    }

    #[test]
    fn test_insert_dotted() {
        let mut table = toml::map::Map::new();
        insert_dotted(&mut table, "a.b.c", toml::Value::Integer(42));

        let a = table.get("a").unwrap().as_table().unwrap();
        let b = a.get("b").unwrap().as_table().unwrap();
        assert_eq!(b.get("c").unwrap().as_integer(), Some(42));
    }

    #[test]
    fn a_key_with_an_underscore_in_its_name_stays_one_key() {
        let source = EnvVarSource::new("FUNDAMENTAL_").with_vars(EnvVars::fixed([(
            "FUNDAMENTAL_DATABASE__SQL__MAX_CONNECTIONS",
            "20",
        )]));

        let loaded = source
            .load()
            .expect("the source loads")
            .expect("one variable is not nothing");
        let sql = loaded
            .get("database")
            .and_then(toml::Value::as_table)
            .and_then(|database| database.get("sql"))
            .and_then(toml::Value::as_table)
            .expect("the two declared levels are tables");

        assert_eq!(
            sql.get("max_connections").and_then(toml::Value::as_integer),
            Some(20),
            "the pool was not sized; keys: {:?}",
            sql.keys().collect::<Vec<_>>()
        );
        assert!(
            !sql.contains_key("max"),
            "a pool named `max` was invented out of the separator"
        );
    }
}
