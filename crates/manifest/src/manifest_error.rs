//! Why a project's settings could not be read, queried or written.

use fundamental_core::AnyError;
use fundamental_core::Error as _;
use fundamental_macros::Error;

use crate::ManifestFinding;

/// Why a manifest could not be read, queried or written.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ManifestError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// A file changed between the editor reading it and writing it.
    #[error(
        code = "MANIFEST_009",
        message = "{file} changed while it was being edited; run the command again to edit the file as it is now",
        status = 409,
        retryable
    )]
    ChangedSinceRead {
        /// The file, relative to the project.
        file: String,
    },

    /// A table does not have the shape its reader expects.
    #[error(
        code = "MANIFEST_010",
        message = "`{key}` ({at}) does not have the expected shape: {cause}",
        status = 500
    )]
    Shape {
        /// The key that was read.
        key: String,
        /// Where it is defined, as `path:line:column` for each file.
        at: String,
        /// What did not fit.
        cause: String,
    },

    /// No `fun.toml` was found.
    #[error(
        code = "MANIFEST_011",
        message = "no fun.toml in {start} or any directory above it",
        status = 404
    )]
    NotFound {
        /// Where the search started.
        start: String,
    },

    /// A key is not a TOML key, or names nothing a file can hold.
    #[error(
        code = "MANIFEST_012",
        message = "`{key}` cannot be used: {cause}",
        status = 400
    )]
    BadKey {
        /// The key as given.
        key: String,
        /// Why it was refused.
        cause: String,
    },

    /// A file could not be read or written.
    #[error(
        code = "MANIFEST_013",
        message = "{path} could not be read or written: {cause}",
        status = 500
    )]
    Io {
        /// The file or directory.
        path: String,
        /// The operating system's reason.
        cause: String,
    },

    /// The manifest was read and refused, for every reason listed.
    #[error(
        code = "MANIFEST_014",
        message = "the project's settings are refused ({count} finding(s)):\n{listing}",
        status = 500
    )]
    Refused {
        /// How many findings there are.
        count: usize,
        /// Every finding, one per line.
        listing: String,
        /// The findings themselves, for a caller that reads their codes.
        #[error(skip_param)]
        findings: Vec<ManifestFinding>,
    },
}

impl ManifestError {
    /// A refusal carrying `findings`, listed one per line in its message.
    pub(crate) fn refused(findings: Vec<ManifestFinding>) -> Self {
        let listing = findings
            .iter()
            .map(|finding| format!("  {}: {finding}", finding.error_code()))
            .collect::<Vec<_>>()
            .join("\n");
        Self::Refused {
            count: findings.len(),
            listing,
            findings,
        }
    }

    pub(crate) fn io(path: &std::path::Path, error: &std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            cause: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_changed_file_is_retryable() {
        let changed = ManifestError::ChangedSinceRead { file: "a".into() };
        assert_eq!(changed.error_code(), "MANIFEST_009");
        assert!(changed.is_retryable());
        let others = [
            (
                ManifestError::Shape {
                    key: "k".into(),
                    at: "a".into(),
                    cause: "c".into(),
                },
                "MANIFEST_010",
            ),
            (
                ManifestError::NotFound { start: "/".into() },
                "MANIFEST_011",
            ),
            (
                ManifestError::BadKey {
                    key: "k".into(),
                    cause: "c".into(),
                },
                "MANIFEST_012",
            ),
            (
                ManifestError::Io {
                    path: "p".into(),
                    cause: "c".into(),
                },
                "MANIFEST_013",
            ),
            (ManifestError::refused(Vec::new()), "MANIFEST_014"),
        ];
        for (error, code) in others {
            assert_eq!(error.error_code(), code, "{error}");
            assert!(!error.is_retryable(), "{error}");
        }
    }

    #[test]
    fn a_refusal_lists_every_finding_with_its_code() {
        let error = ManifestError::refused(vec![
            ManifestFinding::Renamed {
                old: "fundamental.toml".into(),
                new: "fun.toml".into(),
            },
            ManifestFinding::DevelopmentFile {
                file: "fun.development.toml".into(),
            },
        ]);
        let text = error.to_string();
        assert!(text.contains("2 finding(s)"), "{text}");
        assert!(text.contains("MANIFEST_017: `fundamental.toml`"), "{text}");
        assert!(
            text.contains("MANIFEST_016: fun.development.toml"),
            "{text}"
        );
    }
}
