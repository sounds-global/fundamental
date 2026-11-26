//! Every trybuild harness is a `compile_fail` binary, so `.config/nextest.toml`'s
//! `cargo-builds` group — whose filter names that binary — runs it one at a time.
//!
//! Two trybuild cases of one crate share cargo's lock, charging one build to the other's 300 s budget; a nextest filter cannot report a harness it misses, so the name is checked here.

use std::fs;
use std::path::{Path, PathBuf};

const TEST_ROOTS: [&str; 2] = ["crates", "cli"];

/// This file names the marker as a fixture, not as a call.
const THIS_CHECKER: &str = "every_trybuild_harness_is_bounded";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn test_files(root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![root.join("tests")];
    if let Ok(packages) = fs::read_dir(root) {
        dirs.extend(
            packages
                .flatten()
                .map(|package| package.path().join("tests")),
        );
    }
    dirs.into_iter()
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect()
}

/// The files among `files` that use trybuild under a name the group's filter does not match.
fn unbounded_harnesses(files: &[PathBuf]) -> Vec<String> {
    files
        .iter()
        .filter(|path| {
            path.file_stem()
                .is_some_and(|stem| stem != "compile_fail" && stem != THIS_CHECKER)
        })
        .filter(|path| fs::read_to_string(path).is_ok_and(|text| text.contains("trybuild::")))
        .map(|path| path.display().to_string())
        .collect()
}

#[test]
fn every_trybuild_harness_is_bounded() {
    let files: Vec<PathBuf> = TEST_ROOTS
        .iter()
        .flat_map(|root| test_files(&workspace().join(root)))
        .collect();
    assert!(
        files
            .iter()
            .any(|path| path.ends_with("tests/compile_fail.rs")),
        "the scan found no compile_fail harness; it is not looking where they are"
    );
    let unbounded = unbounded_harnesses(&files);
    assert!(
        unbounded.is_empty(),
        "trybuild used outside a `compile_fail` binary runs unbounded: {unbounded:?}"
    );
}

#[test]
fn a_harness_under_another_name_is_named() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let planted = dir.path().join("ui.rs");
    fs::write(&planted, "fn t() { trybuild::TestCases::new(); }").expect("a planted harness");
    let bounded = dir.path().join("compile_fail.rs");
    fs::write(&bounded, "fn t() { trybuild::TestCases::new(); }").expect("a bounded harness");
    assert_eq!(
        unbounded_harnesses(&[planted.clone(), bounded]),
        [planted.display().to_string()]
    );
}
