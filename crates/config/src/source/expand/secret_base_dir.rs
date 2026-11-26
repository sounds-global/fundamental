use std::path::{Path, PathBuf};

/// Where a relative `${file:…}` reference resolves from: the project root, not the working directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretBaseDir(PathBuf);

impl SecretBaseDir {
    /// Resolves relative references against `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self(root.into())
    }

    /// The absolute path a `${file:…}` reference names; an empty base keeps it relative to the cwd.
    pub(crate) fn resolve(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            return path.to_path_buf();
        }
        self.0.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absolute_reference_ignores_the_base() {
        let base = SecretBaseDir::new("/srv/acme");
        assert_eq!(
            base.resolve("/run/secrets/issuer_key"),
            PathBuf::from("/run/secrets/issuer_key")
        );
    }

    #[test]
    fn a_relative_reference_resolves_against_the_base() {
        let base = SecretBaseDir::new("/srv/acme");
        assert_eq!(
            base.resolve("secrets/issuer_key"),
            PathBuf::from("/srv/acme/secrets/issuer_key")
        );
    }

    #[test]
    fn an_empty_base_keeps_the_reference_relative_to_the_working_directory() {
        assert_eq!(
            SecretBaseDir::default().resolve("secrets/issuer_key"),
            PathBuf::from("secrets/issuer_key")
        );
    }
}
