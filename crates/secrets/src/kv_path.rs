//! [`KvPath`] — where one key-value (version 2) secret lives.

use std::fmt;

use fundamental_config::settings::is_secret_path;

use crate::error::{SecretError, SecretResult};

/// The mount and path of one key-value (version 2) secret, held to the secret-path alphabet.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct KvPath {
    mount: String,
    path: String,
}

impl KvPath {
    /// The secret at `path` under the key-value engine mounted at `mount`, slashes trimmed.
    ///
    /// # Errors
    /// [`SecretError::InvalidConfig`] when either part holds an empty or `..` segment, or a
    /// character outside the path alphabet.
    pub fn new(mount: &str, path: &str) -> SecretResult<Self> {
        Ok(Self {
            mount: segments(mount, "mount")?.to_owned(),
            path: segments(path, "path")?.to_owned(),
        })
    }

    /// The engine's mount.
    #[must_use]
    pub fn mount(&self) -> &str {
        &self.mount
    }

    /// The secret's path under the mount.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn data(&self) -> String {
        format!("{}/data/{}", self.mount, self.path)
    }

    pub(crate) fn metadata(&self) -> String {
        format!("{}/metadata/{}", self.mount, self.path)
    }

    pub(crate) fn destroy(&self) -> String {
        format!("{}/destroy/{}", self.mount, self.path)
    }
}

impl fmt::Display for KvPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.mount, self.path)
    }
}

impl fmt::Debug for KvPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KvPath({self})")
    }
}

/// `value` as segments of a URL path, or the refusal naming which `part` it was.
pub(crate) fn segments<'a>(value: &'a str, part: &str) -> SecretResult<&'a str> {
    let trimmed = value.trim_matches('/');
    if is_secret_path(trimmed) {
        Ok(trimmed)
    } else {
        Err(SecretError::InvalidConfig(format!(
            "{value:?} is not a secret {part}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_names_its_data_metadata_and_destroy_endpoints() {
        let path = KvPath::new("/a7d/", "/intake/t/s/test/").expect("valid");
        assert_eq!(path.data(), "a7d/data/intake/t/s/test");
        assert_eq!(path.metadata(), "a7d/metadata/intake/t/s/test");
        assert_eq!(path.destroy(), "a7d/destroy/intake/t/s/test");
        assert_eq!(path.to_string(), "a7d/intake/t/s/test");
    }

    #[test]
    fn a_path_that_could_name_another_secret_is_refused() {
        for (mount, path) in [
            ("", "p"),
            ("m", ""),
            ("m", "a/../b"),
            ("m", "a//b"),
            ("m/..", "p"),
        ] {
            let refused = KvPath::new(mount, path);
            assert!(
                matches!(refused, Err(SecretError::InvalidConfig(_))),
                "{mount:?} {path:?}"
            );
        }
    }
}
