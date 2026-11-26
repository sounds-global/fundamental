//! [`UnpackedPackage`] — a package unpacked, and what it held.

use std::path::PathBuf;

use crate::PackageManifest;

/// A package unpacked, and what it held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpackedPackage {
    /// The directory its files were unpacked into, each at its packed path.
    pub root: PathBuf,
    /// The BLAKE3 of the tar.zst, as 64 lowercase hexadecimal digits.
    pub digest: String,
    /// Bytes of the tar.zst.
    pub packed: u64,
    /// Bytes of every file it held, `package.json` included.
    pub unpacked: u64,
    /// Its `package.json`.
    pub manifest: PackageManifest,
}
