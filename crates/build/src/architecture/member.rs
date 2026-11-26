//! [`Member`], one crate of the workspace.

use std::path::PathBuf;

use super::dependency::Dependency;

/// One crate of the workspace, read from its `Cargo.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    /// The package name, which `[architecture]` places.
    pub(crate) package: String,
    /// The name source code uses for it: its `[lib] name`, or the package name with `-` as `_`.
    pub(crate) ident: String,
    /// Its directory, relative to the project.
    pub(crate) dir: PathBuf,
    /// Every crate it depends on, development dependencies included and marked.
    pub(crate) dependencies: Vec<Dependency>,
}

impl Member {
    /// Its `Cargo.toml`, relative to the project.
    pub(crate) fn manifest(&self) -> PathBuf {
        self.dir.join("Cargo.toml")
    }
}
