//! TOML file config source ([`TomlFileSource`]).

use crate::{ConfigError, ConfigResult, ConfigSource};
use std::fs;
use std::path::PathBuf;
use tracing::debug;

/// Loads configuration from a TOML file.
#[derive(Debug)]
pub struct TomlFileSource {
    path: PathBuf,
    required: bool,
}

impl TomlFileSource {
    /// Creates a TOML file source; a missing file is an error only when `required`.
    pub fn new(path: impl Into<PathBuf>, required: bool) -> Self {
        Self {
            path: path.into(),
            required,
        }
    }

    /// Creates an optional source.
    pub fn optional(path: impl Into<PathBuf>) -> Self {
        Self::new(path, false)
    }
}

impl ConfigSource for TomlFileSource {
    fn name(&self) -> &str {
        "toml-file"
    }

    fn load(&self) -> ConfigResult<Option<toml::Value>> {
        if !self.path.exists() {
            if self.required {
                return Err(ConfigError::ReadFile {
                    path: self.path.clone(),
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
                });
            }
            debug!(path = %self.path.display(), "optional config file not found; skipping");
            return Ok(None);
        }

        let content = fs::read_to_string(&self.path).map_err(|e| ConfigError::ReadFile {
            path: self.path.clone(),
            source: e,
        })?;

        let value: toml::Value = toml::from_str(&content).map_err(|e| ConfigError::ParseToml {
            path: self.path.clone(),
            message: e.to_string(),
        })?;

        debug!(path = %self.path.display(), "loaded config from file");
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn optional_missing_file_logs_a_structured_skip() {
        let source = TomlFileSource::optional("/nonexistent/fundamental-test-config.toml");
        let loaded = source
            .load()
            .expect("optional missing file is not an error");

        assert!(loaded.is_none());
        assert!(
            logs_contain("optional config file not found"),
            "a skipped optional config file must be logged with its path"
        );
    }
}
