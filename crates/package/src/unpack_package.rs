//! [`unpack_package`] — a package's files, unpacked only as its `package.json` names them.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use tracing::info;

use crate::bounded_reader::BoundedReader;
use crate::hashing_reader::HashingReader;
use crate::past_the_bound::PastTheBound;
use crate::{PackageBounds, PackageError, PackageManifest, PackedFile, UnpackedPackage};

/// The most `package.json` may hold: far more than 10 000 entries' paths and hashes need.
const MANIFEST_LIMIT: u64 = 8 << 20;

/// What tar adds per entry at most: its header, a GNU long-name header and the name's block (a
/// packed path is at most 255 bytes), and the padding of its data to a block.
const ENTRY_OVERHEAD: u64 = 4 * 512;

/// What tar adds after the last entry at most: the two zero blocks and a record's padding.
const ARCHIVE_TAIL: u64 = 2 * 512 + 20 * 512;

/// Unpacks the package at `package` into `into`, a directory this creates, only when its BLAKE3
/// is `digest` and it holds exactly what its `package.json` names; a refused package unpacks
/// nothing.
///
/// # Errors
/// [`PackageError::NotAFile`] when `package` is a link or not a regular file;
/// [`PackageError::TooLarge`] past `bounds`; [`PackageError::DigestRefused`] for another
/// package; [`PackageError::EntryRefused`] for an entry that is not the next one
/// `package.json` names; [`PackageError::ManifestRefused`] and [`PackageError::PathRefused`]
/// for a `package.json` that does not parse or a path a package may not hold;
/// [`PackageError::Io`], also when `into` already exists.
pub fn unpack_package(
    package: &Path,
    digest: &str,
    into: &Path,
    bounds: PackageBounds,
) -> Result<UnpackedPackage, PackageError> {
    let mut file = open_regular(package)?;
    let packed = verify(&mut file, package, digest, bounds.packed)?;
    fs::create_dir(into).map_err(|cause| PackageError::io(into, cause))?;
    let unpacked = file
        .seek(SeekFrom::Start(0))
        .map_err(|cause| PackageError::io(package, cause))
        .and_then(|_| unpack(&file, package, into, bounds));
    let (manifest, unpacked) = match unpacked {
        Ok(read) => read,
        Err(error) => {
            let _ = fs::remove_dir_all(into);
            return Err(error);
        }
    };
    info!(package = %package.display(), packed, unpacked, files = manifest.files().len(), "package unpacked");
    Ok(UnpackedPackage {
        root: into.to_path_buf(),
        digest: digest.to_owned(),
        packed,
        unpacked,
        manifest,
    })
}

/// The regular file at `path`, never through a link.
fn open_regular(path: &Path) -> Result<File, PackageError> {
    fundamental_core::fs::open_regular(path).map_err(|cause| match cause.kind() {
        io::ErrorKind::InvalidInput => PackageError::NotAFile {
            source_path: path.display().to_string(),
        },
        _ => PackageError::io(path, cause),
    })
}

/// The package's bytes, once its size is within `limit` and its BLAKE3 is `digest`.
fn verify(file: &mut File, path: &Path, digest: &str, limit: u64) -> Result<u64, PackageError> {
    let size = file
        .metadata()
        .map_err(|cause| PackageError::io(path, cause))?
        .len();
    if size > limit {
        return Err(too_large("packed bytes", limit));
    }
    let mut hasher = blake3::Hasher::new();
    hasher
        .update_reader(BoundedReader::new(&mut *file, limit))
        .map_err(|cause| read_error(path, "packed bytes", limit, cause))?;
    let found = hasher.finalize().to_hex().to_string();
    if found != digest {
        return Err(PackageError::DigestRefused {
            expected: digest.to_owned(),
            found,
        });
    }
    Ok(hasher.count())
}

/// Reads the archive in `file` into `into`, answering its manifest and unpacked bytes.
fn unpack(
    file: &File,
    path: &Path,
    into: &Path,
    bounds: PackageBounds,
) -> Result<(PackageManifest, u64), PackageError> {
    let decoded_limit = bounds
        .unpacked
        .saturating_add((bounds.entries as u64).saturating_mul(ENTRY_OVERHEAD))
        .saturating_add(ARCHIVE_TAIL);
    let decoder = zstd::Decoder::new(file).map_err(|cause| PackageError::io(path, cause))?;
    let mut archive = tar::Archive::new(BoundedReader::new(decoder, decoded_limit));
    let failed = |cause: io::Error| read_error(path, "unpacked bytes", bounds.unpacked, cause);
    let mut entries = archive.entries().map_err(failed)?;

    let mut first = entries
        .next()
        .ok_or_else(|| refused(PackageManifest::PATH, "is not the package's first entry"))?
        .map_err(failed)?;
    let manifest = read_manifest(&mut first, bounds)?;
    let unpacked = manifest
        .files()
        .iter()
        .fold(first.size(), |sum, packed| sum.saturating_add(packed.size));
    drop(first);

    for packed in manifest.files() {
        let mut entry = entries
            .next()
            .ok_or_else(|| {
                refused(
                    packed.path.as_str(),
                    "is named by package.json but not packed",
                )
            })?
            .map_err(failed)?;
        check_entry(&entry, packed)?;
        write_file(&mut entry, packed, into)?;
    }
    if let Some(extra) = entries.next() {
        let extra = extra.map_err(failed)?;
        return Err(refused(&entry_path(&extra), "is not named by package.json"));
    }
    Ok((manifest, unpacked))
}

/// The first entry's `package.json`, within its own bound and the package's.
fn read_manifest(
    entry: &mut tar::Entry<'_, impl Read>,
    bounds: PackageBounds,
) -> Result<PackageManifest, PackageError> {
    let path = entry_path(entry);
    if path != PackageManifest::PATH {
        return Err(refused(
            PackageManifest::PATH,
            "is not the package's first entry",
        ));
    }
    if entry.header().entry_type() != tar::EntryType::Regular {
        return Err(refused(&path, "is not a regular file"));
    }
    if entry.size() > MANIFEST_LIMIT.min(bounds.unpacked) {
        return Err(too_large("unpacked bytes", bounds.unpacked));
    }
    let mut json = Vec::new();
    entry
        .read_to_end(&mut json)
        .map_err(|cause| PackageError::io(Path::new(PackageManifest::PATH), cause))?;
    let manifest = PackageManifest::parse(&json)?;
    if manifest.files().len() + 1 > bounds.entries {
        return Err(too_large("entries", bounds.entries as u64));
    }
    let unpacked = manifest
        .files()
        .iter()
        .fold(json.len() as u64, |sum, packed| {
            sum.saturating_add(packed.size)
        });
    if unpacked > bounds.unpacked {
        return Err(too_large("unpacked bytes", bounds.unpacked));
    }
    Ok(manifest)
}

/// Refuses an entry that is not `packed`: another path, kind, size or mode.
fn check_entry(entry: &tar::Entry<'_, impl Read>, packed: &PackedFile) -> Result<(), PackageError> {
    let path = entry_path(entry);
    if path != packed.path.as_str() {
        return Err(refused(&path, "is not the next file package.json names"));
    }
    let header = entry.header();
    if header.entry_type() != tar::EntryType::Regular {
        return Err(refused(&path, "is not a regular file"));
    }
    if entry.size() != packed.size {
        return Err(refused(&path, "is not the size package.json names"));
    }
    if header.mode().ok() != Some(packed.mode.bits()) {
        return Err(refused(&path, "is not the mode package.json names"));
    }
    Ok(())
}

/// Writes `entry`'s bytes new at its packed path under `into`, refusing bytes whose BLAKE3 is
/// not the one `package.json` names.
fn write_file(
    entry: &mut tar::Entry<'_, impl Read>,
    packed: &PackedFile,
    into: &Path,
) -> Result<(), PackageError> {
    use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};

    let target = into.join(packed.path.as_str());
    if let Some(parent) = target.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(parent)
            .map_err(|cause| PackageError::io(parent, cause))?;
    }
    let io = |cause: io::Error| PackageError::io(&target, cause);
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(packed.mode.bits())
        .open(&target)
        .map_err(io)?;
    let mut reader = HashingReader::new(entry.take(packed.size));
    io::copy(&mut reader, &mut out).map_err(io)?;
    if reader.hex() != packed.blake3 {
        return Err(refused(
            packed.path.as_str(),
            "is not the bytes package.json names",
        ));
    }
    out.set_permissions(fs::Permissions::from_mode(packed.mode.bits()))
        .map_err(io)?;
    out.sync_all().map_err(io)
}

/// The entry's path as the archive holds it, `..` and all, for comparing and naming.
fn entry_path(entry: &tar::Entry<'_, impl Read>) -> String {
    String::from_utf8_lossy(&entry.path_bytes()).into_owned()
}

fn refused(path: &str, reason: &'static str) -> PackageError {
    PackageError::EntryRefused {
        path: path.to_owned(),
        reason,
    }
}

fn too_large(measure: &'static str, limit: u64) -> PackageError {
    PackageError::TooLarge { measure, limit }
}

/// The bound, when a bounded read refused past it; else the read's own failure.
fn read_error(path: &Path, measure: &'static str, limit: u64, cause: io::Error) -> PackageError {
    if PastTheBound::caused(&cause) {
        too_large(measure, limit)
    } else {
        PackageError::io(path, cause)
    }
}
