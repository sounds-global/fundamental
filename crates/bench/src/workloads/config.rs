//! A project's settings loaded: `fun.toml` with `fun.production.toml` over it.

use std::path::{Path, PathBuf};

use fundamental_config::ConfigLoader;
use fundamental_manifest::Manifest;
use tempfile::TempDir;

const FUN_TOML: &str = r#"
[project]
name = "bench"

[apps.main]
web = { host = "127.0.0.1", port = 3000 }

[apps.main.client]
api_url = "https://api.example.test"

[apps.main.database.sql]
url = "postgresql://bench@127.0.0.1:5433/bench"
max_connections = 20

[apps.main.messaging.nats]
url = "nats://127.0.0.1:4222"
"#;

const PRODUCTION_TOML: &str = r#"
[apps.main]
web = { host = "0.0.0.0", port = 8080 }
"#;

/// A project directory holding `fun.toml` and `fun.production.toml`.
#[derive(Debug)]
pub struct ConfigLoad {
    dir: TempDir,
    fun_toml: PathBuf,
}

impl ConfigLoad {
    /// The project, written to a temporary directory.
    ///
    /// # Errors
    ///
    /// The I/O error when the directory or a file cannot be written.
    pub fn new() -> std::io::Result<Self> {
        let dir = tempfile::tempdir()?;
        let fun_toml = dir.path().join("fun.toml");
        std::fs::write(&fun_toml, FUN_TOML)?;
        std::fs::write(dir.path().join("fun.production.toml"), PRODUCTION_TOML)?;
        Ok(Self { dir, fun_toml })
    }

    /// The project's directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// The configuration production loads; the port it serves on, or none when refused.
    #[must_use]
    pub fn load(&self) -> Option<u16> {
        ConfigLoader::production()
            .with_fundamental_toml(&self.fun_toml)
            .load()
            .ok()
            .map(|config| config.server.port)
    }

    /// The settings as `fun manifest` reads them for production; how many keys.
    #[must_use]
    pub fn manifest(&self) -> Option<usize> {
        Manifest::load_for(self.root(), "production")
            .ok()
            .map(|manifest| manifest.as_table().len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_is_loaded_over_the_project() {
        let config = ConfigLoad::new().expect("written");
        assert_eq!(config.load(), Some(8080));
        assert!(config.manifest().is_some_and(|keys| keys > 0));
    }
}
