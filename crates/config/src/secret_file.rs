//! [`SecretFile`]: write key material to disk, owner-readable only.

use std::path::{Path, PathBuf};

use crate::{ConfigError, ConfigResult};

/// A file holding key material, written `0600` (the mode is skipped off unix).
///
/// ```rust,no_run
/// # extern crate fundamental_config;
/// use fundamental_config::SecretFile;
///
/// # fn main() -> fundamental_config::ConfigResult<()> {
/// SecretFile::at("keys/layer0-auth-private.pem").write(pem())?;
/// # Ok(())
/// # }
/// # fn pem() -> &'static str { "" }
/// ```
#[derive(Debug, Clone)]
pub struct SecretFile {
    path: PathBuf,
}

impl SecretFile {
    /// Targets `path`.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The path this writes to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the file already exists.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Writes `contents` atomically and owner-only, creating any missing parent directories.
    ///
    /// # Errors
    /// [`ConfigError::SecretFileUnreadable`], naming the path and never the contents.
    pub fn write(&self, contents: &str) -> ConfigResult<()> {
        self.create_parent()?;
        crate::AtomicFile::at(&self.path)
            .mode(0o600)
            .replace(contents.as_bytes())
            .map_err(|error| match error {
                ConfigError::FileNotReplaced { message, .. } => ConfigError::SecretFileUnreadable {
                    path: self.path.display().to_string(),
                    message,
                },
                other => other,
            })
    }

    fn create_parent(&self) -> ConfigResult<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|error| self.failed(error))?;
        }
        Ok(())
    }

    /// Writes only when the file does not already exist, in one atomic create; returns whether it wrote.
    ///
    /// # Errors
    /// As [`write`](Self::write); an already-present file is `Ok(false)`.
    pub fn write_if_absent(&self, contents: &str) -> ConfigResult<bool> {
        self.create_parent()?;

        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        match options.open(&self.path) {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(contents.as_bytes())
                    .map_err(|error| self.failed(error))?;
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(self.failed(error)),
        }
    }

    fn failed(&self, error: std::io::Error) -> ConfigError {
        ConfigError::SecretFileUnreadable {
            path: self.path.display().to_string(),
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_secret_is_owner_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = SecretFile::at(dir.path().join("nested/key.pem"));

        file.write("-----BEGIN PRIVATE KEY-----\n").expect("writes");

        assert_eq!(
            std::fs::read_to_string(file.path()).expect("reads"),
            "-----BEGIN PRIVATE KEY-----\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file.path())
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "mode was {:o}", mode & 0o777);
        }
    }

    #[cfg(unix)]
    #[test]
    fn overwriting_a_world_readable_file_tightens_it() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("leaky.pem");
        std::fs::write(&path, "old").expect("seed");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");

        SecretFile::at(&path).write("new").expect("writes");

        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "mode was {:o}", mode & 0o777);
    }

    #[cfg(unix)]
    #[test]
    fn a_secret_created_by_write_if_absent_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let file = SecretFile::at(dir.path().join("nested/fresh.pem"));

        assert!(
            file.write_if_absent("-----BEGIN PRIVATE KEY-----\n")
                .expect("writes")
        );

        let mode = std::fs::metadata(file.path())
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "mode was {:o}", mode & 0o777);
    }

    #[test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn only_one_concurrent_minter_creates_the_secret() {
        use std::sync::{Arc, Barrier};

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("contended.pem");

        const MINTERS: usize = 16;
        // Released together, so the threads are inside the call at the same moment.
        let gate = Arc::new(Barrier::new(MINTERS));

        let winners: Vec<String> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..MINTERS)
                .map(|n| {
                    let gate = Arc::clone(&gate);
                    let path = path.clone();
                    scope.spawn(move || {
                        let mine = format!("key-from-minter-{n}");
                        gate.wait();
                        SecretFile::at(&path)
                            .write_if_absent(&mine)
                            .expect("no minter fails")
                            .then_some(mine)
                    })
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().expect("minter thread"))
                .collect()
        });

        assert_eq!(
            winners.len(),
            1,
            "{} minters believed they created the key; only one may",
            winners.len()
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("reads"),
            winners[0],
            "the bytes on disk belong to a minter that was told it did not write"
        );
    }

    #[test]
    fn write_if_absent_leaves_an_existing_secret_alone() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = SecretFile::at(dir.path().join("key.pem"));

        assert!(file.write_if_absent("first").expect("writes"));
        assert!(!file.write_if_absent("second").expect("skips"));
        assert_eq!(
            std::fs::read_to_string(file.path()).expect("reads"),
            "first"
        );
    }
}
