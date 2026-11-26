//! The [`write_binary_file`] helper: write raw bytes, creating parent directories.

use std::fs;
use std::path::Path;

use crate::CliError;

/// Write `bytes` to `path`, creating any missing parent directories.
///
/// # Errors
/// [`CliError::Io`] if a parent directory or the file cannot be written.
pub(crate) fn write_binary_file(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CliError::Io {
            path: parent.display().to_string(),
            reason: e.to_string(),
        })?;
    }
    fs::write(path, bytes).map_err(|e| CliError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::write_binary_file;

    #[test]
    fn writes_bytes_creating_parents() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("a").join("b.bin");
        write_binary_file(&path, &[1, 2, 3]).expect("writes");
        assert_eq!(std::fs::read(&path).expect("reads"), vec![1, 2, 3]);
    }
}
