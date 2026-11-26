//! The lock every manifest editor holds while it writes.

use std::fs::{self, File, OpenOptions};
use std::path::Path;

use tracing::debug;

use crate::ManifestError;

/// An exclusive lock on a project's manifest, held for as long as the value lives; in its own
/// file, since a lock on the manifest would be lost when it is renamed over.
pub(crate) struct EditLock {
    file: File,
}

impl EditLock {
    /// Waits for, then takes, the lock of the manifest in `source_dir`.
    pub(crate) fn acquire(source_dir: &Path) -> Result<Self, ManifestError> {
        let directory = source_dir.join(".fun");
        fs::create_dir_all(&directory).map_err(|error| ManifestError::io(&directory, &error))?;
        let path = directory.join("manifest.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|error| ManifestError::io(&path, &error))?;
        debug!(lock = %path.display(), "waiting for the manifest lock");
        file.lock()
            .map_err(|error| ManifestError::io(&path, &error))?;
        Ok(Self { file })
    }
}

impl Drop for EditLock {
    fn drop(&mut self) {
        if let Err(error) = self.file.unlock() {
            debug!(%error, "manifest lock released by closing it instead");
        }
    }
}
