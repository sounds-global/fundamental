//! [`hash_file`] — a regular file's size and BLAKE3.

use std::fs::{self, File};
use std::path::Path;

use crate::PackageError;

/// The size and BLAKE3 (64 lowercase hexadecimal digits) of the regular file at `path`.
///
/// # Errors
/// [`PackageError::NotAFile`] for a link, a directory or a special file;
/// [`PackageError::Io`] when it cannot be read.
pub(crate) fn hash_file(path: &Path) -> Result<(u64, String), PackageError> {
    let regular = fs::symlink_metadata(path)
        .map_err(|cause| PackageError::io(path, cause))?
        .file_type()
        .is_file();
    if !regular {
        return Err(PackageError::NotAFile {
            source_path: path.display().to_string(),
        });
    }
    let file = File::open(path).map_err(|cause| PackageError::io(path, cause))?;
    let mut hasher = blake3::Hasher::new();
    hasher
        .update_reader(file)
        .map_err(|cause| PackageError::io(path, cause))?;
    Ok((hasher.count(), hasher.finalize().to_hex().to_string()))
}
