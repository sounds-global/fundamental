//! Configuration sources ([`ConfigSource`]).

mod dev_vars;
mod env_var;
mod expand;
mod fundamental_toml;
mod project_manifest;
mod toml_file;
mod validate;

pub use env_var::EnvVarSource;
pub use fundamental_toml::FundamentalTomlSource;
pub use toml_file::TomlFileSource;

pub(crate) use dev_vars::load_dev_vars;
pub use expand::*;
pub(crate) use project_manifest::project_manifest;
pub(crate) use validate::*;

use super::ConfigResult;

/// A source of configuration values; later sources override earlier ones.
pub trait ConfigSource {
    /// Returns the name of this source for debugging.
    fn name(&self) -> &str;

    /// Loads configuration values from this source, or `None` if it has none.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    fn load(&self) -> ConfigResult<Option<toml::Value>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConfigError;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_toml_file_source() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[server]\nport = 8080\n").unwrap();

        let source = TomlFileSource::new(&path, true);
        let value = source.load().unwrap().unwrap();

        let table = value.as_table().unwrap();
        let server = table.get("server").unwrap().as_table().unwrap();
        assert_eq!(server.get("port").unwrap().as_integer(), Some(8080));
    }

    #[test]
    fn test_toml_file_source_optional_missing() {
        let source = TomlFileSource::optional("/nonexistent/config.toml");
        let value = source.load().unwrap();
        assert!(value.is_none());
    }

    #[test]
    fn test_toml_file_source_required_missing() {
        let source = TomlFileSource::new("/nonexistent/config.toml", true);
        let result = source.load();
        assert!(result.is_err());
    }

    #[test]
    fn a_file_reference_reads_the_secret_without_touching_the_environment() {
        let path = std::env::temp_dir().join("fundamental_file_directive_secret");
        std::fs::write(&path, "s3cret-value\n").expect("write the secret file");

        let expanded = expand_env_vars(
            &format!("${{file:{}}}", path.display()),
            &SecretBaseDir::default(),
        )
        .expect("read the secret");

        assert_eq!(expanded, "s3cret-value");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_relative_file_reference_resolves_against_the_project_root() {
        let root = std::env::temp_dir().join("fundamental_relative_secret_root");
        std::fs::create_dir_all(root.join("secrets")).expect("create the secrets directory");
        std::fs::write(root.join("secrets").join("issuer_key"), "from-the-root\n")
            .expect("write the secret file");

        let expanded = expand_env_vars(
            "${file:secrets/issuer_key}",
            &SecretBaseDir::new(root.clone()),
        )
        .expect("the reference resolves against the project root, not the cwd");

        assert_eq!(expanded, "from-the-root");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_missing_file_reference_falls_back_to_its_default() {
        let expanded = expand_env_vars(
            "${file:/nonexistent/secret:-fallback}",
            &SecretBaseDir::default(),
        )
        .expect("the default covers an absent optional secret");
        assert_eq!(expanded, "fallback");
    }

    #[test]
    fn a_missing_file_reference_without_a_default_is_an_error() {
        let error = expand_env_vars("${file:/nonexistent/secret}", &SecretBaseDir::default())
            .expect_err("an absent required secret must fail the load");

        assert!(
            matches!(&error, ConfigError::SecretFileUnreadable { path, .. }
                if path == "/nonexistent/secret"),
            "expected SecretFileUnreadable, got {error:?}"
        );
    }

    #[test]
    fn test_expand_env_vars_missing() {
        let result = expand_env_vars("${NONEXISTENT_VAR_123}", &SecretBaseDir::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_expand_env_vars_with_default() {
        let result = expand_env_vars(
            "${NONEXISTENT_VAR_456:-default_value}",
            &SecretBaseDir::default(),
        )
        .unwrap();
        assert_eq!(result, "default_value");
    }

    #[test]
    fn test_expand_env_vars_default_with_url() {
        let result = expand_env_vars(
            "${API_URL_TEST:-http://127.0.0.1:8001}",
            &SecretBaseDir::default(),
        )
        .unwrap();
        assert_eq!(result, "http://127.0.0.1:8001");
    }

    #[test]
    fn test_expand_env_vars_multiple_defaults() {
        let result =
            expand_env_vars("${VAR_A:-aaa}/${VAR_B:-bbb}", &SecretBaseDir::default()).unwrap();
        assert_eq!(result, "aaa/bbb");
    }
}
