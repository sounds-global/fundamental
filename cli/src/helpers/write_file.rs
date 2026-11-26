//! The [`write_file`] helper: write a file, creating parent directories.

use std::fs;
use std::path::Path;

use crate::CliError;

/// Write `contents` to `path`, creating any missing parent directories.
///
/// # Errors
/// [`CliError::Io`] if a parent directory or the file cannot be written.
pub(crate) fn write_file(path: &Path, contents: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CliError::Io {
            path: parent.display().to_string(),
            reason: e.to_string(),
        })?;
    }
    fs::write(path, contents).map_err(|e| CliError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}
