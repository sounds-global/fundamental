//! An app's built files as one bounded, verifiable package: a tar.zst whose first entry,
//! `package.json` ([`PackageManifest`]), names every other one.
//!
//! [`write_package`] makes one from files on disk; [`unpack_package`] reads one back only when
//! it is the one expected, and only as its `package.json` names it.

mod bounded_reader;
mod file_mode;
mod hash_file;
mod hashing_reader;
mod package_bounds;
mod package_entry;
mod package_error;
mod package_manifest;
mod package_profile;
mod packed_file;
mod packed_path;
mod past_the_bound;
#[cfg(unix)]
mod unpack_package;
mod unpacked_package;
mod write_package;
mod written_package;

pub use file_mode::FileMode;
pub use package_bounds::PackageBounds;
pub use package_entry::PackageEntry;
pub use package_error::PackageError;
pub use package_manifest::PackageManifest;
pub use package_profile::PackageProfile;
pub use packed_file::PackedFile;
pub use packed_path::PackedPath;
#[cfg(unix)]
pub use unpack_package::unpack_package;
pub use unpacked_package::UnpackedPackage;
pub use write_package::write_package;
pub use written_package::WrittenPackage;
