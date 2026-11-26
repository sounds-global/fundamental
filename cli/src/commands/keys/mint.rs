//! A key version written to its own file, never over another.

use std::path::{Path, PathBuf};

use crate::CliError;

/// Writes a new owner-only key for `purpose` at `version` to `<secrets>/<purpose>/<version>.key`.
pub(super) fn mint_key_file(
    secrets: &Path,
    purpose: &str,
    version: u16,
) -> Result<PathBuf, CliError> {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
    if purpose.is_empty() || purpose.starts_with('.') || !purpose.chars().all(plain) {
        return Err(CliError::KeyPurposeInvalid {
            purpose: purpose.to_owned(),
        });
    }
    let dir = secrets.join(purpose);
    let path = dir.join(format!("{version}.key"));
    let unwritable = |reason: std::io::Error| CliError::KeyUnwritable {
        path: path.display().to_string(),
        reason: reason.to_string(),
    };
    std::fs::create_dir_all(&dir).map_err(unwritable)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = match options.open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(CliError::KeyExists {
                path: path.display().to_string(),
            });
        }
        Err(error) => return Err(unwritable(error)),
    };
    let material = fundamental_encryption::mint_labelled_material(purpose, version);
    std::io::Write::write_all(&mut file, material.as_bytes())
        .and_then(|()| std::io::Write::write_all(&mut file, b"\n"))
        .map_err(unwritable)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use fundamental::Error as _;

    use super::mint_key_file;

    #[test]
    fn a_key_is_written_labelled_for_its_purpose_and_version() {
        let dir = tempfile::tempdir().expect("a directory");
        let path = mint_key_file(dir.path(), "example.live-handle.v1", 2).expect("written");
        assert_eq!(
            path,
            dir.path().join("example.live-handle.v1").join("2.key")
        );
        let text = std::fs::read_to_string(&path).expect("readable");
        let (label, hex) = text.trim().rsplit_once(':').expect("labelled");
        assert_eq!(label, "example.live-handle.v1:2");
        assert_eq!(hex.len(), 64);
    }

    #[cfg(unix)]
    #[test]
    fn only_its_owner_may_read_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("a directory");
        let path = mint_key_file(dir.path(), "example.v1", 1).expect("written");
        let mode = std::fs::metadata(&path)
            .expect("there")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_key_is_never_written_over() {
        let dir = tempfile::tempdir().expect("a directory");
        let path = mint_key_file(dir.path(), "example.v1", 1).expect("written");
        let first = std::fs::read_to_string(&path).expect("readable");
        let refused = mint_key_file(dir.path(), "example.v1", 1).expect_err("exists");
        assert_eq!(refused.error_code(), "CLI_073");
        assert_eq!(std::fs::read_to_string(&path).expect("readable"), first);
    }

    #[test]
    fn a_purpose_that_names_a_path_is_refused() {
        let dir = tempfile::tempdir().expect("a directory");
        for purpose in ["../escape", "a/b", "", "."] {
            assert!(mint_key_file(dir.path(), purpose, 1).is_err(), "{purpose}");
        }
    }
}
