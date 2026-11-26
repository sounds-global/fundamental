use std::env;
use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::{InstanceKey, LocalError, RootFingerprint};

/// Where this machine's local origins live: `$XDG_STATE_HOME/fundamental/local`, or
/// `~/.local/state/fundamental/local`, mode `0700`, never inside a checkout (root keys live here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalState {
    dir: PathBuf,
}

impl LocalState {
    /// The state directory at `dir`, for tests and for a machine that keeps it elsewhere.
    #[must_use]
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The developer's state directory.
    ///
    /// # Errors
    ///
    /// [`LocalError::UnsupportedPlatform`] (`LOCAL_013`) when neither `XDG_STATE_HOME` nor
    /// `HOME` names an absolute directory.
    pub fn for_user() -> Result<Self, LocalError> {
        let base = env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|home| home.join(".local").join("state"))
            })
            .ok_or_else(|| LocalError::UnsupportedPlatform {
                detail: "neither XDG_STATE_HOME nor HOME names an absolute directory".to_owned(),
            })?;
        Ok(Self::at(base.join("fundamental").join("local")))
    }

    /// The directory itself.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The registry, the one mutable source of truth.
    #[must_use]
    pub fn registry(&self) -> PathBuf {
        self.dir.join("registry.toml")
    }

    /// The file whose exclusive lock serialises every change; it is never replaced.
    #[must_use]
    pub fn lock(&self) -> PathBuf {
        self.dir.join("registry.lock")
    }

    /// Where the root with `fingerprint` and its key are kept.
    #[must_use]
    pub fn root_dir(&self, fingerprint: &RootFingerprint) -> PathBuf {
        self.dir.join("roots").join(fingerprint.to_string())
    }

    /// Where one instance's leaf and edge configuration are kept.
    #[must_use]
    pub fn instance_dir(&self, key: &InstanceKey) -> PathBuf {
        self.dir
            .join("instances")
            .join(key.project().as_str())
            .join(key.slot())
    }

    /// The one directory an instance's edge container is given.
    #[must_use]
    pub fn edge_dir(&self, key: &InstanceKey) -> PathBuf {
        self.instance_dir(key).join("edge")
    }

    /// The directory the machine resolver reads its `Corefile` from.
    #[must_use]
    pub fn resolver_dir(&self) -> PathBuf {
        self.dir.join("resolver")
    }

    /// Creates the resolver's directory, mode `0755`, inside the private state directory.
    ///
    /// `0755` because the resolver's capability-less root reads a bind mount only through the
    /// "other" bits; it holds only the `Corefile`, no secret.
    ///
    /// # Errors
    ///
    /// [`LocalError::Io`] when the directory cannot be created or its mode set.
    pub fn resolver_dir_readable(&self) -> Result<PathBuf, LocalError> {
        let dir = self.resolver_dir();
        self.private_dir(&dir)?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755))
            .map_err(|cause| LocalError::io(&dir, "made readable by the resolver", cause))?;
        Ok(dir)
    }

    /// Creates `path` and every directory between it and the state directory with mode `0700`.
    ///
    /// # Errors
    ///
    /// [`LocalError::Io`] when a directory cannot be created or its mode set.
    pub fn private_dir(&self, path: &Path) -> Result<(), LocalError> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|cause| LocalError::io(path, "created", cause))?;
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
            .map_err(|cause| LocalError::io(&self.dir, "made private", cause))
    }
}
