//! [`AtomicFile`]: replace a file whole, or not at all.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tracing::{debug, warn};

use crate::{ConfigError, ConfigResult};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// A file only ever replaced whole: a reader sees the old contents or the new, never half.
///
/// Both the temporary file and its directory are flushed, so a crash never leaves an empty file.
///
/// ```rust,no_run
/// # extern crate fundamental_config;
/// use fundamental_config::AtomicFile;
///
/// # fn main() -> fundamental_config::ConfigResult<()> {
/// AtomicFile::at("volumes/edge/list.json").mode(0o640).replace(b"{}")?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct AtomicFile {
    path: PathBuf,
    mode: u32,
}

impl AtomicFile {
    /// The mode a file gets when none is given: its owner alone.
    pub const DEFAULT_MODE: u32 = 0o600;

    /// Targets `path`, owner-only until [`mode`](Self::mode) says otherwise.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            mode: Self::DEFAULT_MODE,
        }
    }

    /// The permission bits the replacement is created with.
    #[must_use]
    pub const fn mode(mut self, mode: u32) -> Self {
        self.mode = mode;
        self
    }

    /// The path this replaces.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Replace the file with `contents`, creating it if absent; the parent directory must exist.
    ///
    /// # Errors
    /// [`ConfigError::FileNotReplaced`], naming the path and never the contents; the target is left as it was.
    pub fn replace(&self, contents: &[u8]) -> ConfigResult<()> {
        let temporary = self.temporary_path()?;
        if let Err(error) = self.write_temporary(&temporary, contents) {
            remove_temporary(&temporary);
            return Err(error);
        }
        fs::rename(&temporary, &self.path).map_err(|error| {
            remove_temporary(&temporary);
            self.failed(&error)
        })?;
        self.flush_directory()?;
        debug!(path = %self.path.display(), bytes = contents.len(), "file replaced");
        Ok(())
    }

    fn temporary_path(&self) -> ConfigResult<PathBuf> {
        let name = self
            .path
            .file_name()
            .ok_or_else(|| ConfigError::FileNotReplaced {
                path: self.path.display().to_string(),
                message: "the path names no file".to_owned(),
            })?;
        let unique = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let mut temporary = std::ffi::OsString::from(".");
        temporary.push(name);
        temporary.push(format!(".{}.{unique}.writing", std::process::id()));
        Ok(self.path.with_file_name(temporary))
    }

    fn write_temporary(&self, temporary: &Path, contents: &[u8]) -> ConfigResult<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(self.mode);
        }
        let mut file = options
            .open(temporary)
            .map_err(|error| self.failed(&error))?;
        // `mode` is filtered by the umask; the explicit set is what makes it the mode asked for.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(temporary, fs::Permissions::from_mode(self.mode))
                .map_err(|error| self.failed(&error))?;
        }
        file.write_all(contents)
            .map_err(|error| self.failed(&error))?;
        file.sync_all().map_err(|error| self.failed(&error))
    }

    fn flush_directory(&self) -> ConfigResult<()> {
        #[cfg(unix)]
        if let Some(parent) = self.path.parent() {
            let directory = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            File::open(directory)
                .and_then(|handle| handle.sync_all())
                .map_err(|error| self.failed(&error))?;
        }
        Ok(())
    }

    fn failed(&self, error: &std::io::Error) -> ConfigError {
        ConfigError::FileNotReplaced {
            path: self.path.display().to_string(),
            message: error.to_string(),
        }
    }
}

fn remove_temporary(temporary: &Path) {
    if let Err(cleanup) = fs::remove_file(temporary) {
        warn!(path = %temporary.display(), %cleanup, "a temporary file was left behind");
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use super::AtomicFile;
    use crate::ConfigError;

    #[test]
    fn a_file_is_replaced_whole() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = AtomicFile::at(dir.path().join("list.json"));
        file.replace(b"first").expect("created");
        file.replace(b"second, longer than the first")
            .expect("replaced");
        assert_eq!(
            std::fs::read(file.path()).expect("reads"),
            b"second, longer than the first"
        );
    }

    #[test]
    fn nothing_but_the_target_is_left_in_the_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = AtomicFile::at(dir.path().join("list.json"));
        for round in 0..5 {
            file.replace(format!("round {round}").as_bytes())
                .expect("replaced");
        }
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .expect("lists")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(names, ["list.json"]);
    }

    #[cfg(unix)]
    #[test]
    fn the_replacement_has_the_mode_asked_for_whatever_was_there() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.json");
        std::fs::write(&path, b"lax").expect("a lax file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).expect("lax");

        AtomicFile::at(&path).replace(b"tight").expect("replaced");
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "owner-only by default");

        AtomicFile::at(&path)
            .mode(0o640)
            .replace(b"shared")
            .expect("replaced");
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[test]
    fn a_missing_directory_is_refused_and_named() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("absent/list.json");
        let error = AtomicFile::at(&path)
            .replace(b"x")
            .expect_err("no directory is created");
        assert!(
            matches!(&error, ConfigError::FileNotReplaced { path: named, .. }
                if *named == path.display().to_string()),
            "{error:?}"
        );
    }

    fn replace_repeatedly(path: &Path, payload: &[u8]) {
        for _ in 0..20 {
            AtomicFile::at(path).replace(payload).expect("replaced");
        }
    }

    #[test]
    fn concurrent_writers_leave_one_whole_payload() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = Arc::new(dir.path().join("list.json"));
        let payloads: Vec<Vec<u8>> = (0..8_u8).map(|writer| vec![writer; 64 * 1024]).collect();
        AtomicFile::at(path.as_ref())
            .replace(&payloads[0])
            .expect("seeded");

        let writers: Vec<_> = payloads
            .iter()
            .cloned()
            .map(|payload| {
                let path = Arc::clone(&path);
                std::thread::spawn(move || replace_repeatedly(&path, &payload))
            })
            .collect();
        for _ in 0..200 {
            let read = std::fs::read(path.as_ref()).expect("reads");
            assert!(
                payloads.contains(&read),
                "a reader saw {} bytes that are no writer's payload",
                read.len()
            );
        }
        for writer in writers {
            writer.join().expect("writer");
        }
    }
}
