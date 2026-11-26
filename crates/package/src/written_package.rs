//! [`WrittenPackage`] — a package made, and what it holds.

use std::path::PathBuf;

use crate::PackageManifest;

/// A package made, and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenPackage {
    /// Where it was written.
    pub path: PathBuf,
    /// The BLAKE3 of the tar.zst, as 64 lowercase hexadecimal digits.
    pub digest: String,
    /// Bytes of the tar.zst.
    pub packed: u64,
    /// Bytes of every file it holds, `package.json` included.
    pub unpacked: u64,
    /// Its `package.json`.
    pub manifest: PackageManifest,
}
