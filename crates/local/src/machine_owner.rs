use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tracing::{info, warn};

use crate::machine_files::OWNER_FILE;
use crate::{LocalError, LocalState};

/// The one user whose local origins this machine runs, as `setup` recorded it.
#[derive(Debug, Clone, Copy)]
pub struct MachineOwner;

#[derive(Debug, Deserialize)]
struct OwnerFile {
    owner: u32,
    #[serde(default)]
    state: Option<PathBuf>,
}

impl MachineOwner {
    /// Refuses `euid` when another user set this machine up.
    ///
    /// # Errors
    ///
    /// [`LocalError::NotMachineOwner`] (`LOCAL_018`) when the owner file names another uid;
    /// [`LocalError::Io`] when it exists and cannot be read.
    pub fn check(euid: u32) -> Result<(), LocalError> {
        Self::check_at(Path::new(OWNER_FILE), euid)
    }

    fn check_at(path: &Path, euid: u32) -> Result<(), LocalError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(LocalError::io(path, "read", error)),
        };
        match toml::from_str::<OwnerFile>(&text) {
            Ok(file) if file.owner == euid => Ok(()),
            Ok(file) => Err(LocalError::NotMachineOwner { owner: file.owner }),
            Err(error) => {
                warn!(path = %path.display(), %error, "the owner file does not parse; `fun local setup` rewrites it");
                Ok(())
            }
        }
    }
}

impl MachineOwner {
    /// Whether the machine's resolver serves the registry in `state`: the one `setup` was run
    /// against, or any while the machine is not set up.
    ///
    /// A run against another registry (a test's) must leave the machine-wide resolver alone.
    #[must_use]
    pub fn serves(state: &LocalState) -> bool {
        Self::serves_at(Path::new(OWNER_FILE), state)
    }

    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn serves_at(path: &Path, state: &LocalState) -> bool {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return true,
            Err(error) => {
                warn!(path = %path.display(), %error, "local.resolver: the owner file cannot be read, so no registry is served");
                return false;
            }
        };
        match toml::from_str::<OwnerFile>(&text) {
            Ok(OwnerFile {
                state: Some(served),
                ..
            }) if served == state.dir() => true,
            Ok(OwnerFile {
                state: Some(served),
                ..
            }) => {
                info!(served = %served.display(), this = %state.dir().display(), "local.resolver: the machine's resolver serves another registry; it is left alone");
                false
            }
            Ok(OwnerFile { state: None, .. }) => {
                warn!(path = %path.display(), "local.resolver: the owner file names no registry; run `fun local setup` again");
                false
            }
            Err(error) => {
                warn!(path = %path.display(), %error, "local.resolver: the owner file does not parse, so no registry is served; `fun local setup` rewrites it");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::MachineOwner;
    use crate::LocalState;

    #[test]
    fn the_resolver_serves_only_the_registry_setup_recorded() {
        let dir = tempfile::tempdir().expect("a directory");
        let path = dir.path().join("local.toml");
        let own = LocalState::at(dir.path().join("own"));
        let scratch = LocalState::at(dir.path().join("scratch"));
        assert!(
            MachineOwner::serves_at(&path, &scratch),
            "a machine nobody set up has no registry to protect"
        );
        std::fs::write(
            &path,
            format!("owner = 1001\nstate = \"{}\"\n", own.dir().display()),
        )
        .expect("an owner");
        assert!(MachineOwner::serves_at(&path, &own));
        assert!(!MachineOwner::serves_at(&path, &scratch));
        std::fs::write(&path, "owner = 1001\n").expect("an older owner file");
        assert!(
            !MachineOwner::serves_at(&path, &own),
            "an owner file naming no registry serves none until setup runs again"
        );
    }

    #[test]
    fn another_users_machine_is_refused() {
        let dir = tempfile::tempdir().expect("a directory");
        let path = dir.path().join("local.toml");
        assert!(MachineOwner::check_at(&path, 1001).is_ok(), "no owner yet");
        std::fs::write(&path, "owner = 1001\n").expect("an owner");
        assert!(MachineOwner::check_at(&path, 1001).is_ok());
        let refused = MachineOwner::check_at(&path, 1002).map_err(|error| error.error_code());
        assert_eq!(refused, Err("LOCAL_018"));
    }
}
