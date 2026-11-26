//! [`PackageManifest`] — a package's `package.json`: what it is and every file it holds.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{PackageError, PackageProfile, PackedFile};

/// A package's `package.json`, its first entry: which app, built for what, from which commit,
/// and every other file it holds with its size, mode and BLAKE3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    /// Always [`FORMAT`](Self::FORMAT).
    format: String,
    /// Always [`VERSION`](Self::VERSION).
    version: u32,
    /// The app the package runs.
    app: String,
    /// What it was built for.
    profile: PackageProfile,
    /// The commit it was built from.
    commit: String,
    /// Every file it holds besides `package.json`, by path.
    files: Vec<PackedFile>,
}

impl PackageManifest {
    /// The entry that holds it: always a package's first.
    pub const PATH: &'static str = "package.json";
    /// What a package's `format` says.
    pub const FORMAT: &'static str = "fundamental-package";
    /// The format's version.
    pub const VERSION: u32 = 1;

    /// A package of `app`'s `files`, built for `profile` from `commit`; the files are held by
    /// path.
    ///
    /// # Errors
    /// [`PackageError::ManifestRefused`] for an app that is not `[a-z][a-z0-9_-]{0,63}` or a
    /// commit that is not 7 to 64 lowercase hexadecimal digits; [`PackageError::PackedTwice`]
    /// for two files at one path.
    pub fn new(
        app: &str,
        profile: PackageProfile,
        commit: &str,
        mut files: Vec<PackedFile>,
    ) -> Result<Self, PackageError> {
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let manifest = Self {
            format: Self::FORMAT.to_owned(),
            version: Self::VERSION,
            app: app.to_owned(),
            profile,
            commit: commit.to_owned(),
            files,
        };
        manifest.checked()
    }

    /// The manifest of `entries`, each read once for its size and BLAKE3.
    ///
    /// # Errors
    /// As [`new`](Self::new); [`PackageError::NotAFile`] for an entry that is not a regular
    /// file; [`PackageError::Io`] for one that cannot be read.
    pub fn measure(
        app: &str,
        profile: PackageProfile,
        commit: &str,
        entries: &[crate::PackageEntry],
    ) -> Result<Self, PackageError> {
        let files = entries
            .iter()
            .map(|entry| {
                let (size, blake3) = crate::hash_file::hash_file(&entry.source)?;
                Ok(PackedFile {
                    path: entry.path.clone(),
                    size,
                    mode: entry.mode,
                    blake3,
                })
            })
            .collect::<Result<Vec<_>, PackageError>>()?;
        Self::new(app, profile, commit, files)
    }

    /// The `package.json` in `bytes`, as a package's first entry holds it.
    ///
    /// # Errors
    /// [`PackageError::ManifestRefused`] when it is not JSON of this shape, names another format
    /// or version, or an app or commit [`new`](Self::new) refuses; [`PackageError::PathRefused`]
    /// for a path a package may not hold; [`PackageError::PackedTwice`].
    pub fn parse(bytes: &[u8]) -> Result<Self, PackageError> {
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|e| PackageError::ManifestRefused {
                field: "package.json",
                reason: e.to_string(),
            })?;
        if manifest.format != Self::FORMAT || manifest.version != Self::VERSION {
            return Err(PackageError::ManifestRefused {
                field: "format",
                reason: format!(
                    "'{}' version {} is not {} version {}",
                    manifest.format,
                    manifest.version,
                    Self::FORMAT,
                    Self::VERSION
                ),
            });
        }
        manifest.checked()
    }

    /// The manifest as `package.json` holds it.
    ///
    /// # Panics
    /// Never: every field serialises.
    #[must_use]
    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).expect("a manifest serialises")
    }

    /// The app the package runs.
    #[must_use]
    pub fn app(&self) -> &str {
        &self.app
    }

    /// What it was built for.
    #[must_use]
    pub const fn profile(&self) -> PackageProfile {
        self.profile
    }

    /// The commit it was built from.
    #[must_use]
    pub fn commit(&self) -> &str {
        &self.commit
    }

    /// Every file it holds besides `package.json`, by path.
    #[must_use]
    pub fn files(&self) -> &[PackedFile] {
        &self.files
    }

    fn checked(self) -> Result<Self, PackageError> {
        let name = |byte: u8| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        };
        let app = self.app.as_bytes();
        if app.is_empty()
            || app.len() > 64
            || !app[0].is_ascii_lowercase()
            || !app.iter().copied().all(name)
        {
            return Err(PackageError::ManifestRefused {
                field: "app",
                reason: format!("'{}' is not [a-z][a-z0-9_-]{{0,63}}", self.app),
            });
        }
        let hex = |byte: u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte);
        if !(7..=64).contains(&self.commit.len()) || !self.commit.bytes().all(hex) {
            return Err(PackageError::ManifestRefused {
                field: "commit",
                reason: format!(
                    "'{}' is not 7 to 64 lowercase hexadecimal digits",
                    self.commit
                ),
            });
        }
        let mut seen = BTreeSet::new();
        for file in &self.files {
            if !seen.insert(file.path.as_str()) {
                return Err(PackageError::PackedTwice {
                    path: file.path.to_string(),
                });
            }
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;
    use crate::{FileMode, PackedPath};

    fn file(path: &str) -> PackedFile {
        PackedFile {
            path: PackedPath::parse(path).expect("a path"),
            size: 3,
            mode: FileMode::Plain,
            blake3: "0".repeat(64),
        }
    }

    #[test]
    fn a_manifest_reads_back_as_it_was_written() {
        let written = PackageManifest::new(
            "preview_sample",
            PackageProfile::Preview,
            "457b125",
            vec![file("share/dist/app.css"), file("bin/preview_sample")],
        )
        .expect("a manifest");

        let read = PackageManifest::parse(&written.to_json()).expect("read back");

        assert_eq!(read, written);
        assert_eq!(
            read.files()[0].path.as_str(),
            "bin/preview_sample",
            "held by path"
        );
    }

    #[test]
    fn two_files_at_one_path_are_refused() {
        let err = PackageManifest::new(
            "preview_sample",
            PackageProfile::Preview,
            "457b125",
            vec![file("bin/x"), file("bin/x")],
        )
        .expect_err("twice");

        assert_eq!(err.error_code(), "PKG_004");
    }

    #[test]
    fn an_app_or_commit_a_package_may_not_name_is_refused() {
        for (app, commit, field) in [
            ("../x", "457b125", "app"),
            ("Preview", "457b125", "app"),
            ("preview_sample", "HEAD", "commit"),
            ("preview_sample", "457b12", "commit"),
        ] {
            let err = PackageManifest::new(app, PackageProfile::Preview, commit, vec![])
                .expect_err("refused");

            assert!(
                matches!(&err, PackageError::ManifestRefused { field: f, .. } if *f == field),
                "{err:?}"
            );
            assert_eq!(err.error_code(), "PKG_006");
        }
    }

    #[test]
    fn another_format_or_an_unknown_field_is_refused_as_it_is_read() {
        let written =
            PackageManifest::new("preview_sample", PackageProfile::Preview, "457b125", vec![])
                .expect("a manifest");
        let json = String::from_utf8(written.to_json()).expect("utf-8");

        for refused in [
            json.replace("\"version\": 1", "\"version\": 2"),
            json.replace("fundamental-package", "other"),
            json.replace("\"files\"", "\"extra\": 1,\n  \"files\""),
        ] {
            let err = PackageManifest::parse(refused.as_bytes()).expect_err(&refused);
            assert_eq!(err.error_code(), "PKG_006", "{refused}");
        }
    }
}
