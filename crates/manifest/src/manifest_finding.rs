//! One thing wrong with a project's settings, found while loading them.

use fundamental_core::AnyError;
use fundamental_macros::Error;

/// One thing wrong with a project's settings files, found while they were read.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ManifestFinding {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// A file is not TOML.
    #[error(
        code = "MANIFEST_001",
        message = "{at} is not TOML: {cause}",
        status = 500
    )]
    Unparseable {
        /// The file and line, as `path:line:column`.
        at: String,
        /// What the parser objected to.
        cause: String,
    },

    /// A file is larger than any settings file needs to be.
    #[error(
        code = "MANIFEST_007",
        message = "{file} is refused: {reason}",
        status = 500
    )]
    TooLarge {
        /// The file past the bound.
        file: String,
        /// Which bound it passed.
        reason: String,
    },

    /// `fun.development.toml` exists, and development is `fun.toml` itself.
    #[error(
        code = "MANIFEST_016",
        message = "{file} is refused: development runs on fun.toml itself; move its values into fun.toml, or into fun.local.toml for this machine alone",
        status = 500
    )]
    DevelopmentFile {
        /// The file, relative to the project.
        file: String,
    },

    /// A file or directory of the old layout, with the name its settings move to.
    #[error(
        code = "MANIFEST_017",
        message = "`{old}` is the old layout; its settings move to {new}",
        status = 500
    )]
    Renamed {
        /// The old file or directory, relative to the project.
        old: String,
        /// Where its settings belong now.
        new: String,
    },

    /// An override turns a table into a value, or a value into a table.
    #[error(
        code = "MANIFEST_018",
        message = "{at} overrides `{key}` with a different kind of value than {base} gives it: an override changes values, and a table stays a table",
        status = 500
    )]
    Override {
        /// The override, as `path:line:column`.
        at: String,
        /// The key both define.
        key: String,
        /// Where the overridden definition is, as `path:line:column`.
        base: String,
    },
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn every_finding_has_its_code_and_is_not_retryable() {
        let findings = [
            (
                ManifestFinding::Unparseable {
                    at: "fun.toml:1:1".into(),
                    cause: "c".into(),
                },
                "MANIFEST_001",
            ),
            (
                ManifestFinding::TooLarge {
                    file: "fun.toml".into(),
                    reason: "r".into(),
                },
                "MANIFEST_007",
            ),
            (
                ManifestFinding::DevelopmentFile {
                    file: "fun.development.toml".into(),
                },
                "MANIFEST_016",
            ),
            (
                ManifestFinding::Renamed {
                    old: "fundamental.toml".into(),
                    new: "fun.toml".into(),
                },
                "MANIFEST_017",
            ),
            (
                ManifestFinding::Override {
                    at: "fun.local.toml:1:1".into(),
                    key: "k".into(),
                    base: "fun.toml:1:1".into(),
                },
                "MANIFEST_018",
            ),
        ];
        for (finding, code) in findings {
            assert_eq!(finding.error_code(), code, "{finding}");
            assert!(!finding.is_retryable(), "{finding}");
        }
    }
}
