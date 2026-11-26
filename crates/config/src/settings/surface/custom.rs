//! Application-defined custom settings ([`CustomSettings`]).

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;

use crate::{ConfigError, ConfigResult};

/// Custom application-specific configuration, a flat dot-notation store of TOML values.
///
/// ```rust,ignore
/// // Access values
/// let checkout_enabled = config.custom.get_bool("feature_flags.new_checkout")
///     .unwrap_or(false);
///
/// // Or deserialize into a struct
/// #[derive(Deserialize)]
/// struct PaymentConfig {
///     provider: String,
///     sandbox: bool,
/// }
/// let payment: PaymentConfig = config.custom.section("payment")?;
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CustomSettings {
    values: HashMap<String, toml::Value>,
}

impl CustomSettings {
    /// Creates new empty custom settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    /// Creates custom settings from a TOML value.
    #[must_use]
    pub fn from_toml(value: toml::Value) -> Self {
        let values = if let toml::Value::Table(table) = value {
            flatten_toml_table("", table)
        } else {
            HashMap::new()
        };
        Self { values }
    }

    /// Gets a string value by dot-notation key (e.g. "payment.provider").
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.values.get(key).and_then(|v| v.as_str())
    }

    /// Gets an integer value by key.
    #[must_use]
    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.values.get(key).and_then(|v| v.as_integer())
    }

    /// Gets a boolean value by key.
    #[must_use]
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.values.get(key).and_then(|v| v.as_bool())
    }

    /// Gets a float value by key.
    #[must_use]
    pub fn get_float(&self, key: &str) -> Option<f64> {
        self.values.get(key).and_then(|v| v.as_float())
    }

    /// Gets a raw TOML value by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&toml::Value> {
        self.values.get(key)
    }

    /// Deserializes the values under the top-level key `key` into a typed struct.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// #[derive(Deserialize)]
    /// struct PaymentConfig {
    ///     provider: String,
    ///     sandbox: bool,
    /// }
    ///
    /// let payment: PaymentConfig = config.custom.section("payment")?;
    /// ```
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> ConfigResult<T> {
        let prefix = format!("{key}.");
        let mut table = toml::map::Map::new();

        for (k, v) in &self.values {
            if let Some(suffix) = k.strip_prefix(&prefix) {
                insert_nested(&mut table, suffix, v.clone());
            } else if k == key {
                return T::deserialize(v.clone())
                    .map_err(|e| ConfigError::custom_section(key, e.to_string()));
            }
        }

        if table.is_empty() {
            return Err(ConfigError::missing_field(format!("custom.{key}")));
        }

        T::deserialize(toml::Value::Table(table))
            .map_err(|e| ConfigError::custom_section(key, e.to_string()))
    }

    /// Merges another set of custom settings into this one; `other` wins.
    pub fn merge(&mut self, other: CustomSettings) {
        self.values.extend(other.values);
    }

    /// Sets a value by key.
    pub fn set(&mut self, key: impl Into<String>, value: toml::Value) {
        self.values.insert(key.into(), value);
    }

    /// Returns an iterator over all keys.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }

    /// Returns true if there are no custom settings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

fn flatten_toml_table(
    prefix: &str,
    table: toml::map::Map<String, toml::Value>,
) -> HashMap<String, toml::Value> {
    let mut result = HashMap::new();

    for (key, value) in table {
        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };

        match value {
            toml::Value::Table(nested) => {
                result.extend(flatten_toml_table(&full_key, nested));
            }
            _ => {
                result.insert(full_key, value);
            }
        }
    }

    result
}

fn insert_nested(table: &mut toml::map::Map<String, toml::Value>, path: &str, value: toml::Value) {
    let parts: Vec<&str> = path.splitn(2, '.').collect();

    match parts.as_slice() {
        [key] => {
            table.insert((*key).to_string(), value);
        }
        [key, rest] => {
            let entry = table
                .entry((*key).to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));

            if let toml::Value::Table(nested) = entry {
                insert_nested(nested, rest, value);
            }
        }
        _ => {}
    }
}
