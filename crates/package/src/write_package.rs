//! [`write_package`] — a manifest's files, packed as one tar.zst.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;

use tracing::info;

use crate::hashing_reader::HashingReader;
use crate::past_the_bound::PastTheBound;
use crate::{FileMode, PackageBounds, PackageEntry, PackageError, PackageManifest, WrittenPackage};

/// Most of the gain, at a fraction of the top levels' time.
const LEVEL: i32 = 10;

/// Packs `manifest`'s files from `entries` into a tar.zst at `out`, `package.json` first; `out`
/// never holds a part of one.
///
/// # Errors
/// [`PackageError::TooLarge`] past `bounds`; [`PackageError::ManifestRefused`] when the manifest
/// names a file no entry gives; [`PackageError::ChangedWhilePacking`] when a file's bytes are no
/// longer the ones measured; [`PackageError::NotAFile`] and [`PackageError::Io`].
pub fn write_package(
    manifest: &PackageManifest,
    entries: &[PackageEntry],
    out: &Path,
    bounds: PackageBounds,
) -> Result<WrittenPackage, PackageError> {
    let json = manifest.to_json();
    let count = manifest.files().len() + 1;
    if count > bounds.entries {
        return Err(too_large("entries", bounds.entries as u64));
    }
    let unpacked = manifest
        .files()
        .iter()
        .fold(json.len() as u64, |sum, file| sum.saturating_add(file.size));
    if unpacked > bounds.unpacked {
        return Err(too_large("unpacked bytes", bounds.unpacked));
    }
    let sources: BTreeMap<&str, &Path> = entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.source.as_path()))
        .collect();

    let partial = out.with_extension("partial");
    let written = pack(manifest, &json, &sources, &partial, bounds.packed).and_then(|sink| {
        fs::rename(&partial, out).map_err(|cause| PackageError::io(out, cause))?;
        Ok(sink)
    });
    let (packed, digest) = match written {
        Ok(sink) => sink,
        Err(error) => {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
    };
    info!(package = %out.display(), packed, unpacked, files = count, "package written");
    Ok(WrittenPackage {
        path: out.to_path_buf(),
        digest,
        packed,
        unpacked,
        manifest: manifest.clone(),
    })
}

/// Writes the archive to `path`, answering its bytes and BLAKE3.
fn pack(
    manifest: &PackageManifest,
    json: &[u8],
    sources: &BTreeMap<&str, &Path>,
    path: &Path,
    limit: u64,
) -> Result<(u64, String), PackageError> {
    let file = File::create(path).map_err(|cause| PackageError::io(path, cause))?;
    let sink = BoundedSink::new(BufWriter::new(file), limit);
    let encoder = zstd::Encoder::new(sink, LEVEL).map_err(|cause| PackageError::io(path, cause))?;
    let mut archive = tar::Builder::new(encoder);
    let io = |cause: io::Error| written_error(path, limit, cause);

    append(
        &mut archive,
        PackageManifest::PATH,
        FileMode::Plain,
        json.len() as u64,
        json,
    )
    .map_err(io)?;
    for packed in manifest.files() {
        let source =
            sources
                .get(packed.path.as_str())
                .ok_or_else(|| PackageError::ManifestRefused {
                    field: "files",
                    reason: format!("no entry gives '{}'", packed.path),
                })?;
        let changed = || PackageError::ChangedWhilePacking {
            source_path: source.display().to_string(),
        };
        let (size, _) = crate::hash_file::hash_file(source)?;
        if size != packed.size {
            return Err(changed());
        }
        let file = File::open(source).map_err(|cause| PackageError::io(source, cause))?;
        let mut reader = HashingReader::new(file.take(packed.size));
        append(
            &mut archive,
            packed.path.as_str(),
            packed.mode,
            packed.size,
            &mut reader,
        )
        .map_err(io)?;
        if reader.hex() != packed.blake3 {
            return Err(changed());
        }
    }
    let sink = archive
        .into_inner()
        .and_then(zstd::Encoder::finish)
        .map_err(io)?;
    sink.finish().map_err(io)
}

/// One regular file, owned by root, with `mode` and no time.
fn append(
    archive: &mut tar::Builder<impl Write>,
    path: &str,
    mode: FileMode,
    size: u64,
    data: impl Read,
) -> io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(size);
    header.set_mode(mode.bits());
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    archive.append_data(&mut header, path, data)
}

fn too_large(measure: &'static str, limit: u64) -> PackageError {
    PackageError::TooLarge { measure, limit }
}

/// The bound, when the sink refused a write past it; else the write's own failure.
fn written_error(path: &Path, limit: u64, cause: io::Error) -> PackageError {
    if PastTheBound::caused(&cause) {
        too_large("packed bytes", limit)
    } else {
        PackageError::io(path, cause)
    }
}

/// Counts and hashes what is written, and refuses a write past `limit`.
struct BoundedSink<W> {
    inner: W,
    written: u64,
    limit: u64,
    hasher: blake3::Hasher,
}

impl<W: Write> BoundedSink<W> {
    fn new(inner: W, limit: u64) -> Self {
        Self {
            inner,
            written: 0,
            limit,
            hasher: blake3::Hasher::new(),
        }
    }

    /// Flushes, answering the bytes written and their BLAKE3.
    fn finish(mut self) -> io::Result<(u64, String)> {
        self.inner.flush()?;
        Ok((self.written, self.hasher.finalize().to_hex().to_string()))
    }
}

impl<W: Write> Write for BoundedSink<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.written.saturating_add(bytes.len() as u64) > self.limit {
            return Err(io::Error::other(PastTheBound));
        }
        let wrote = self.inner.write(bytes)?;
        self.hasher.update(&bytes[..wrote]);
        self.written += wrote as u64;
        Ok(wrote)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
