//! A package is unpacked only when it is the one expected, and only as its `package.json` names
//! it: that entry first, then each named file once, in order, a regular file at its size, mode
//! and BLAKE3, and nothing else. Anything else unpacks nothing.
#![cfg(unix)]

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use fundamental_core::Error as _;
use fundamental_package::{
    FileMode, PackageBounds, PackageEntry, PackageError, PackageManifest, PackageProfile,
    PackedPath, WrittenPackage, unpack_package, write_package,
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

fn built(dir: &Path) -> (PackageManifest, WrittenPackage) {
    let entries = vec![
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
    ];
    let manifest = PackageManifest::measure(
        "preview_sample",
        PackageProfile::Preview,
        "457b125",
        &entries,
    )
    .expect("measured");
    let out = dir.join("preview_sample-preview.tar.zst");
    let written =
        write_package(&manifest, &entries, &out, PackageBounds::PREVIEW).expect("written");
    (manifest, written)
}

/// One raw tar entry a hostile package may hold.
struct Raw<'a> {
    path: &'a str,
    kind: tar::EntryType,
    mode: u32,
    bytes: &'a [u8],
    link: Option<&'a str>,
}

impl<'a> Raw<'a> {
    fn file(path: &'a str, mode: u32, bytes: &'a [u8]) -> Self {
        Self {
            path,
            kind: tar::EntryType::Regular,
            mode,
            bytes,
            link: None,
        }
    }
}

/// A tar.zst of exactly `raws`, written by hand as no `write_package` would; answers its BLAKE3.
fn crafted(path: &Path, raws: &[Raw<'_>]) -> String {
    let encoder =
        zstd::Encoder::new(std::fs::File::create(path).expect("create"), 3).expect("zstd");
    let mut archive = tar::Builder::new(encoder);
    for raw in raws {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(raw.kind);
        header.set_size(raw.bytes.len() as u64);
        header.set_mode(raw.mode);
        header.set_mtime(0);
        if let Some(link) = raw.link {
            header.set_link_name(link).expect("a link name");
        }
        // The name's bytes as they are: `set_path` would refuse `..`, as a hostile writer does not.
        header.as_old_mut().name[..raw.path.len()].copy_from_slice(raw.path.as_bytes());
        header.set_cksum();
        archive.append(&header, raw.bytes).expect("append");
    }
    archive
        .into_inner()
        .expect("tar")
        .finish()
        .expect("zstd")
        .flush()
        .expect("flush");
    digest_of(path)
}

fn digest_of(path: &Path) -> String {
    blake3::hash(&std::fs::read(path).expect("read"))
        .to_hex()
        .to_string()
}

/// The `package.json` of one file, `bin/app`, holding `bytes`.
fn one_file_manifest(bytes: &[u8]) -> Vec<u8> {
    let dir = tempfile::tempdir().expect("dir");
    let entries = vec![entry(
        dir.path(),
        "app",
        bytes,
        "bin/app",
        FileMode::Executable,
    )];
    PackageManifest::measure("app", PackageProfile::Preview, "457b125", &entries)
        .expect("measured")
        .to_json()
}

fn into(dir: &Path) -> PathBuf {
    dir.join("unpacked")
}

#[test]
fn a_package_unpacks_as_its_manifest_names_it() {
    let dir = tempfile::tempdir().expect("dir");
    let (manifest, written) = built(dir.path());
    let root = into(dir.path());

    let unpacked = unpack_package(
        &written.path,
        &written.digest,
        &root,
        PackageBounds::PREVIEW,
    )
    .expect("unpacked");

    assert_eq!(unpacked.manifest, manifest);
    assert_eq!(unpacked.root, root);
    assert_eq!(unpacked.digest, written.digest);
    assert_eq!(unpacked.packed, written.packed);
    assert_eq!(unpacked.unpacked, written.unpacked);
    for file in manifest.files() {
        let at = root.join(file.path.as_str());
        let bytes = std::fs::read(&at).expect("unpacked file");
        assert_eq!(
            blake3::hash(&bytes).to_hex().to_string(),
            file.blake3,
            "{}",
            file.path
        );
        let mode = std::fs::metadata(&at)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(mode, file.mode.bits(), "{}", file.path);
    }
    assert!(
        !root.join(PackageManifest::PATH).exists(),
        "package.json is read, not unpacked"
    );
}

#[test]
fn a_package_that_is_not_the_one_expected_unpacks_nothing() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, written) = built(dir.path());
    let root = into(dir.path());
    let other = blake3::hash(b"another package").to_hex().to_string();

    let err =
        unpack_package(&written.path, &other, &root, PackageBounds::PREVIEW).expect_err("another");

    assert!(matches!(err, PackageError::DigestRefused { .. }), "{err:?}");
    assert_eq!(err.error_code(), "PKG_009");
    assert!(!root.exists(), "nothing is unpacked");
}

#[test]
fn a_package_past_its_packed_bound_is_not_read() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, written) = built(dir.path());
    let root = into(dir.path());
    let bounds = PackageBounds {
        packed: 64,
        ..PackageBounds::PREVIEW
    };

    let err = unpack_package(&written.path, &written.digest, &root, bounds).expect_err("too large");

    assert!(
        matches!(
            err,
            PackageError::TooLarge {
                measure: "packed bytes",
                ..
            }
        ),
        "{err:?}"
    );
    assert!(!root.exists());
}

#[test]
fn a_manifest_past_the_unpacked_or_entry_bound_unpacks_nothing() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, written) = built(dir.path());
    for (bounds, measure) in [
        (
            PackageBounds {
                entries: 3,
                ..PackageBounds::PREVIEW
            },
            "entries",
        ),
        (
            PackageBounds {
                unpacked: 64,
                ..PackageBounds::PREVIEW
            },
            "unpacked bytes",
        ),
    ] {
        let root = into(dir.path());

        let err = unpack_package(&written.path, &written.digest, &root, bounds).expect_err(measure);

        assert!(
            matches!(&err, PackageError::TooLarge { measure: m, .. } if *m == measure),
            "{err:?}"
        );
        assert!(!root.exists(), "{measure}: nothing is unpacked");
    }
}

/// Every way a package may differ from the `package.json` it holds, which names one file,
/// `bin/app`: in its files, then in its entries' kinds and paths.
fn hostile(manifest: &[u8]) -> Vec<(&'static str, Vec<Raw<'_>>)> {
    let mut cases = other_files(manifest);
    cases.extend(other_entries(manifest));
    cases
}

fn other_files(manifest: &[u8]) -> Vec<(&'static str, Vec<Raw<'_>>)> {
    let json = || Raw::file("package.json", 0o644, manifest);
    vec![
        (
            "no package.json first",
            vec![Raw::file("bin/app", 0o755, b"the app"), json()],
        ),
        ("a named file missing", vec![json()]),
        (
            "other bytes",
            vec![json(), Raw::file("bin/app", 0o755, b"the ap!")],
        ),
        (
            "another size",
            vec![json(), Raw::file("bin/app", 0o755, b"the app, longer")],
        ),
        (
            "a set-uid mode",
            vec![json(), Raw::file("bin/app", 0o4755, b"the app")],
        ),
        (
            "another path",
            vec![json(), Raw::file("bin/ap", 0o755, b"the app")],
        ),
        (
            "an entry it does not name",
            vec![
                json(),
                Raw::file("bin/app", 0o755, b"the app"),
                Raw::file("bin/more", 0o755, b"more"),
            ],
        ),
    ]
}

fn other_entries(manifest: &[u8]) -> Vec<(&'static str, Vec<Raw<'_>>)> {
    let json = || Raw::file("package.json", 0o644, manifest);
    let raw = |path, kind, mode, bytes: &'static [u8], link| Raw {
        path,
        kind,
        mode,
        bytes,
        link,
    };
    let symlink = tar::EntryType::Symlink;
    vec![
        (
            "a link in place of the file",
            vec![
                json(),
                raw("bin/app", symlink, 0o755, b"", Some("/etc/shadow")),
            ],
        ),
        (
            "a link carrying the file's bytes",
            vec![
                json(),
                raw("bin/app", symlink, 0o755, b"the app", Some("/etc/shadow")),
            ],
        ),
        (
            "a directory before the file",
            vec![
                json(),
                raw("bin", tar::EntryType::Directory, 0o777, b"", None),
                Raw::file("bin/app", 0o755, b"the app"),
            ],
        ),
        (
            "a path leaving its root",
            vec![json(), Raw::file("../bin/app", 0o755, b"the app")],
        ),
    ]
}

#[test]
fn a_package_that_differs_from_its_manifest_unpacks_nothing() {
    let manifest = one_file_manifest(b"the app");

    for (case, raws) in hostile(&manifest) {
        let dir = tempfile::tempdir().expect("dir");
        let package = dir.path().join("p.tar.zst");
        let digest = crafted(&package, &raws);
        let root = into(dir.path());

        let err = unpack_package(&package, &digest, &root, PackageBounds::PREVIEW).expect_err(case);

        assert!(
            matches!(
                err,
                PackageError::EntryRefused { .. } | PackageError::PathRefused { .. }
            ),
            "{case}: {err:?}"
        );
        assert_eq!(err.http_status_code(), 422, "{case}");
        assert!(!root.exists(), "{case}: nothing is unpacked");
        assert!(!dir.path().join("etc").exists(), "{case}");
    }
}

#[test]
fn a_package_is_never_unpacked_over_what_is_there() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, written) = built(dir.path());
    let root = into(dir.path());
    std::fs::create_dir(&root).expect("there already");
    std::fs::write(root.join("kept"), b"kept").expect("write");

    let err = unpack_package(
        &written.path,
        &written.digest,
        &root,
        PackageBounds::PREVIEW,
    )
    .expect_err("there");

    assert!(matches!(err, PackageError::Io { .. }), "{err:?}");
    assert_eq!(
        std::fs::read(root.join("kept")).expect("kept"),
        b"kept",
        "what was there stays"
    );
}

#[test]
fn a_link_to_a_package_is_not_followed() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, written) = built(dir.path());
    let link = dir.path().join("link.tar.zst");
    std::os::unix::fs::symlink(&written.path, &link).expect("link");
    let root = into(dir.path());

    let err =
        unpack_package(&link, &written.digest, &root, PackageBounds::PREVIEW).expect_err("a link");

    assert!(matches!(err, PackageError::NotAFile { .. }), "{err:?}");
    assert!(!root.exists());
}
