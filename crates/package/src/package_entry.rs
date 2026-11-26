//! [`PackageEntry`] — a file on disk, and where and how a package holds it.

use std::path::PathBuf;

use crate::{FileMode, PackedPath};

/// A file on disk, and where and how a package holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEntry {
    /// The file to read.
    pub source: PathBuf,
    /// Where the package holds it.
    pub path: PackedPath,
    /// Whether it runs.
    pub mode: FileMode,
}
