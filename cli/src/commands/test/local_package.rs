//! A package cargo builds from a path on this machine.

use std::path::PathBuf;

/// One package built from a path on this machine: a workspace member or a path dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalPackage {
    pub(super) name: String,
    pub(super) dir: PathBuf,
    /// Whether it is a member of this workspace, so its tests are this run's to run.
    pub(super) member: bool,
    pub(super) depends: Vec<String>,
    /// Globs, relative to the workspace root, of outside files its tests read (`[package.metadata.fun] reads`).
    pub(super) reads: Vec<String>,
}
