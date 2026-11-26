//! [`PackedPath`] — where a package holds a file.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::PackageError;

/// Where a package holds a file: a relative path under `bin/` or `share/`, each part of
/// `A-Z a-z 0-9 . _ - + @ ~` and none of them `.` or `..`, at most 16 parts and 255 bytes. It
/// cannot leave the directory it is unpacked into.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PackedPath(String);

impl PackedPath {
    /// The directories a package's files sit under.
    pub const ROOTS: [&'static str; 2] = ["bin", "share"];

    /// `raw` as a packed path.
    ///
    /// # Errors
    /// [`PackageError::PathRefused`] naming the rule it breaks.
    pub fn parse(raw: &str) -> Result<Self, PackageError> {
        let refuse = |reason| PackageError::PathRefused {
            path: raw.to_owned(),
            reason,
        };
        if raw.len() > 255 {
            return Err(refuse("it is longer than 255 bytes"));
        }
        let parts: Vec<&str> = raw.split('/').collect();
        if parts.len() < 2 || parts.len() > 16 {
            return Err(refuse("it is not two to 16 parts"));
        }
        if !Self::ROOTS.contains(&parts[0]) {
            return Err(refuse("it is not under bin/ or share/"));
        }
        let allowed = |byte: u8| byte.is_ascii_alphanumeric() || b"._-+@~".contains(&byte);
        for part in parts {
            if part.is_empty() || part == "." || part == ".." {
                return Err(refuse("a part is empty, '.' or '..'"));
            }
            if !part.bytes().all(allowed) {
                return Err(refuse(
                    "a part holds a byte outside A-Z a-z 0-9 . _ - + @ ~",
                ));
            }
        }
        Ok(Self(raw.to_owned()))
    }

    /// The path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackedPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for PackedPath {
    type Error = PackageError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(&raw)
    }
}

impl From<PackedPath> for String {
    fn from(path: PackedPath) -> Self {
        path.0
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn a_built_file_s_path_under_a_root_is_packed() {
        for path in [
            "bin/preview_sample",
            "share/dist/preview_sample_bg.wasm",
            "share/dist/assets/fonts/Inter-Var@2x.woff2",
            "share/migrations/sql/0001_init.sql",
        ] {
            assert_eq!(PackedPath::parse(path).expect(path).as_str(), path);
        }
    }

    #[test]
    fn a_path_that_could_leave_its_directory_or_read_two_ways_is_refused() {
        for path in [
            "../etc/passwd",
            "bin/../../etc",
            "bin/./x",
            "/bin/x",
            "bin//x",
            "bin",
            "lib/x",
            "config/fun.toml",
            "fun.toml",
            "fun.production.toml",
            "Cargo.toml",
            "bin/x y",
            "bin/x\\y",
            "share/é",
            &format!("share/{}", "a".repeat(255)),
        ] {
            let err = PackedPath::parse(path).expect_err(path);
            assert_eq!(err.error_code(), "PKG_001", "{path}");
            assert_eq!(err.http_status_code(), 422);
        }
    }

    #[test]
    fn a_manifest_s_path_is_checked_as_it_is_read() {
        let read: Result<PackedPath, _> = serde_json::from_str("\"bin/../x\"");

        assert!(read.is_err());
    }
}
