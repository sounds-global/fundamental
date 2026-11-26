//! [`IslandsCrate`] — the crate an app's browser bundle is built from, when it has one.

use std::path::{Path, PathBuf};

const ISLANDS_DIR: &str = "islands";

/// An app's islands in a crate of their own: `apps/<app>/islands/`, package `<app>_islands`.
///
/// The wasm bundle is then built from it rather than the app crate, so a page edit rebuilds
/// only the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IslandsCrate {
    manifest_path: PathBuf,
    crate_name: String,
}

impl IslandsCrate {
    /// The islands crate of the app named `app` in `app_dir`, or `None` without `islands/Cargo.toml`.
    #[must_use]
    pub fn of_app(app_dir: &Path, app: &str) -> Option<Self> {
        Self::is_in(app_dir).then(|| Self {
            manifest_path: manifest_in(app_dir),
            crate_name: format!("{}_islands", app.replace('-', "_")),
        })
    }

    /// Whether the app in `app_dir` keeps its islands in a crate of their own.
    #[must_use]
    pub fn is_in(app_dir: &Path) -> bool {
        manifest_in(app_dir).is_file()
    }

    /// The crate's `Cargo.toml`.
    #[must_use]
    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    /// The crate's library name, `<app>_islands`.
    #[must_use]
    pub fn crate_name(&self) -> &str {
        &self.crate_name
    }
}

fn manifest_in(app_dir: &Path) -> PathBuf {
    app_dir.join(ISLANDS_DIR).join("Cargo.toml")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn an_app_without_an_islands_directory_has_no_islands_crate() {
        let app = tempfile::tempdir().expect("an app");
        assert_eq!(IslandsCrate::of_app(app.path(), "www"), None);
    }

    #[test]
    fn an_islands_manifest_names_the_crate_after_the_app() {
        let app = tempfile::tempdir().expect("an app");
        fs::create_dir_all(app.path().join("islands")).expect("the islands");
        fs::write(app.path().join("islands/Cargo.toml"), "[package]").expect("manifest");
        let islands = IslandsCrate::of_app(app.path(), "my-site").expect("an islands crate");
        assert_eq!(islands.crate_name(), "my_site_islands");
        assert_eq!(
            islands.manifest_path(),
            app.path().join("islands/Cargo.toml")
        );
    }
}
