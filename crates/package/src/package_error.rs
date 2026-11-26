//! [`PackageError`] — why a package cannot be made or read.

use fundamental_macros::Error;

/// Why a package cannot be made or read.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PackageError {
    /// A path a package would hold is not one it may.
    #[error(
        code = "PKG_001",
        message = "A package cannot hold this path",
        developer_message = "'{path}' cannot be packed: {reason}",
        status = 422
    )]
    PathRefused {
        /// The path.
        path: String,
        /// Why not.
        reason: &'static str,
    },

    /// A file to pack is a link, a directory or a special file.
    #[error(
        code = "PKG_002",
        message = "A package holds regular files only",
        developer_message = "'{source_path}' is not a regular file",
        status = 422
    )]
    NotAFile {
        /// The file on disk.
        source_path: String,
    },

    /// The package would pass one of its bounds.
    #[error(
        code = "PKG_003",
        message = "The package is larger than a package may be",
        developer_message = "the package's {measure} pass {limit}",
        status = 413
    )]
    TooLarge {
        /// `entries`, `unpacked bytes` or `packed bytes`.
        measure: &'static str,
        /// The bound.
        limit: u64,
    },

    /// Two files would be packed at one path.
    #[error(
        code = "PKG_004",
        message = "A package holds each path once",
        developer_message = "'{path}' is packed twice",
        status = 422
    )]
    PackedTwice {
        /// The path.
        path: String,
    },

    /// A file changed between being measured and being packed.
    #[error(
        code = "PKG_005",
        message = "A file changed while it was packed",
        developer_message = "'{source_path}' changed while it was packed; package a finished build",
        status = 409
    )]
    ChangedWhilePacking {
        /// The file on disk.
        source_path: String,
    },

    /// The package's own fields are not ones it may name.
    #[error(
        code = "PKG_006",
        message = "The package names something it may not",
        developer_message = "the package's {field} is refused: {reason}",
        status = 422
    )]
    ManifestRefused {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },

    /// A file could not be read or the package written.
    #[error(
        code = "PKG_007",
        message = "The package could not be read or written",
        developer_message = "'{path}': {cause}",
        status = 500
    )]
    Io {
        /// The file.
        path: String,
        /// The failure.
        #[source]
        cause: std::io::Error,
    },

    /// An entry of a package being unpacked is not the one its `package.json` names next.
    #[error(
        code = "PKG_008",
        message = "The package holds something its package.json does not name",
        developer_message = "'{path}' {reason}",
        status = 422
    )]
    EntryRefused {
        /// The entry's path, as the archive holds it.
        path: String,
        /// How it differs.
        reason: &'static str,
    },

    /// The package to unpack is not the one expected.
    #[error(
        code = "PKG_009",
        message = "The package is not the one expected",
        developer_message = "its BLAKE3 is {found}, not {expected}",
        status = 422
    )]
    DigestRefused {
        /// The BLAKE3 the caller holds for it.
        expected: String,
        /// The BLAKE3 of the bytes read.
        found: String,
    },
}

impl PackageError {
    pub(crate) fn io(path: &std::path::Path, cause: std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            cause,
        }
    }
}
