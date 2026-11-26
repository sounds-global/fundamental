//! A package written from files on disk reads back as exactly its `package.json`: that entry
//! first, then each file once, a regular file owned by root with its mode, its bytes hashing to
//! what the manifest says; and a package past a bound, or of a file that changed, is not written.

use std::io::Read;
use std::path::{Path, PathBuf};

use fundamental_core::Error as _;
use fundamental_package::{
    FileMode, PackageBounds, PackageEntry, PackageError, PackageManifest, PackageProfile,
    PackedPath, write_package,
};

fn entry(dir: &Path, name: &str, bytes: &[u8], path: &str, mode: FileMode) -> PackageEntry {
    let source = dir.join(name);
    std::fs::write(&source, bytes).expect("write source");
    PackageEntry {
        source,
        path: PackedPath::parse(path).expect("a path"),
        mode,
    }
}

fn built(dir: &Path) -> Vec<PackageEntry> {
    vec![
        entry(
            dir,
            "server",
            b"\x7fELF the server",
            "bin/preview_sample",
            FileMode::Executable,
        ),
        entry(
            dir,
            "app.css",
            b"body{}",
            "share/dist/app.css",
            FileMode::Plain,
        ),
        entry(
            dir,
            "0001.sql",
            b"create table t ();",
            "share/migrations/sql/0001_init.sql",
            FileMode::Plain,
        ),
    ]
}

fn measured(entries: &[PackageEntry]) -> PackageManifest {
    PackageManifest::measure(
        "preview_sample",
        PackageProfile::Preview,
        "457b125",
        entries,
    )
    .expect("measured")
}

/// Every entry of the package at `path`: its path, mode, owner and bytes.
fn unpacked(path: &PathBuf) -> Vec<(String, u32, u64, Vec<u8>)> {
    let decoder = zstd::Decoder::new(std::fs::File::open(path).expect("open")).expect("zstd");
    let mut archive = tar::Archive::new(decoder);
    archive
        .entries()
        .expect("entries")
        .map(|entry| {
            let mut entry = entry.expect("an entry");
            assert_eq!(entry.header().entry_type(), tar::EntryType::Regular);
            let path = entry.path().expect("a path").display().to_string();
            let mode = entry.header().mode().expect("a mode");
            let owner = entry.header().uid().expect("a uid") + entry.header().gid().expect("a gid");
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("bytes");
            (path, mode, owner, bytes)
        })
        .collect()
}

#[test]
fn a_package_reads_back_as_its_manifest() {
    let dir = tempfile::tempdir().expect("dir");
    let entries = built(dir.path());
    let manifest = measured(&entries);
    let out = dir.path().join("preview_sample-preview.tar.zst");

    let written =
        write_package(&manifest, &entries, &out, PackageBounds::PREVIEW).expect("written");

    let read = unpacked(&out);
    assert_eq!(read[0].0, PackageManifest::PATH, "package.json comes first");
    assert_eq!(
        PackageManifest::parse(&read[0].3).expect("a manifest"),
        manifest
    );
    let held: Vec<&str> = read[1..].iter().map(|(path, ..)| path.as_str()).collect();
    assert_eq!(
        held,
        [
            "bin/preview_sample",
            "share/dist/app.css",
            "share/migrations/sql/0001_init.sql"
        ]
    );
    for ((path, mode, owner, bytes), file) in read[1..].iter().zip(manifest.files()) {
        assert_eq!(*mode, file.mode.bits(), "{path}");
        assert_eq!(*owner, 0, "{path} is root's");
        assert_eq!(
            blake3::hash(bytes).to_hex().to_string(),
            file.blake3,
            "{path}"
        );
        assert_eq!(bytes.len() as u64, file.size, "{path}");
    }
    let archive = std::fs::read(&out).expect("the package");
    assert_eq!(written.digest, blake3::hash(&archive).to_hex().to_string());
    assert_eq!(written.packed, archive.len() as u64);
}

#[test]
fn a_package_past_a_bound_is_not_written() {
    let dir = tempfile::tempdir().expect("dir");
    let entries = built(dir.path());
    let manifest = measured(&entries);
    let out = dir.path().join("p.tar.zst");
    let noise = incompressible(64 * 1024);
    let mut noisy = entries.clone();
    noisy.push(entry(
        dir.path(),
        "noise",
        &noise,
        "share/dist/noise.bin",
        FileMode::Plain,
    ));
    let noisy_manifest = measured(&noisy);

    for (bounds, entries, manifest, measure) in [
        (
            PackageBounds {
                entries: 3,
                ..PackageBounds::PREVIEW
            },
            &entries,
            &manifest,
            "entries",
        ),
        (
            PackageBounds {
                unpacked: 64,
                ..PackageBounds::PREVIEW
            },
            &entries,
            &manifest,
            "unpacked bytes",
        ),
        (
            PackageBounds {
                packed: 16 * 1024,
                ..PackageBounds::PREVIEW
            },
            &noisy,
            &noisy_manifest,
            "packed bytes",
        ),
    ] {
        let err = write_package(manifest, entries, &out, bounds).expect_err(measure);

        assert!(
            matches!(&err, PackageError::TooLarge { measure: m, .. } if *m == measure),
            "{err:?}"
        );
        assert_eq!(err.error_code(), "PKG_003");
        assert_eq!(err.http_status_code(), 413);
        assert!(!out.exists(), "nothing is left at the package's path");
        assert!(!out.with_extension("partial").exists(), "nor beside it");
    }
}

/// `len` bytes from xorshift64, which zstd cannot shrink.
fn incompressible(len: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect()
}

#[test]
fn a_file_that_changed_after_it_was_measured_is_not_packed() {
    let dir = tempfile::tempdir().expect("dir");
    let entries = built(dir.path());
    let manifest = measured(&entries);
    std::fs::write(&entries[0].source, b"\x7fELF the sErver").expect("changed at the same size");
    let out = dir.path().join("p.tar.zst");

    let err =
        write_package(&manifest, &entries, &out, PackageBounds::PREVIEW).expect_err("changed");

    assert!(
        matches!(err, PackageError::ChangedWhilePacking { .. }),
        "{err:?}"
    );
    assert_eq!(err.error_code(), "PKG_005");
    assert!(!out.exists());
}

#[cfg(unix)]
#[test]
fn a_link_is_not_packed() {
    let dir = tempfile::tempdir().expect("dir");
    let target = dir.path().join("secret");
    std::fs::write(&target, b"not the app's").expect("write");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).expect("link");
    let entries = vec![PackageEntry {
        source: link,
        path: PackedPath::parse("share/dist/app.css").expect("a path"),
        mode: FileMode::Plain,
    }];

    let err = PackageManifest::measure(
        "preview_sample",
        PackageProfile::Preview,
        "457b125",
        &entries,
    )
    .expect_err("a link");

    assert!(matches!(err, PackageError::NotAFile { .. }), "{err:?}");
    assert_eq!(err.error_code(), "PKG_002");
}
