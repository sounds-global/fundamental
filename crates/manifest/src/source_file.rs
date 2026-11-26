//! One manifest file, as read from disk.

use std::path::PathBuf;

/// One manifest file's contents and its path relative to the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceFile {
    pub(crate) file: PathBuf,
    pub(crate) text: String,
}
