//! Every test that drives a real browser sits in a bounded nextest group.
//!
//! A filter matches only names, so an unlisted browser test runs unbounded unnoticed.

use std::fs;
use std::path::{Path, PathBuf};

/// A path into the crate; a file that only names it in prose or a string does not take it.
const TAKES: &str = concat!("fundamental_test_browser", "::");

const GROUPS: [&str; 1] = ["browsers"];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn filters() -> Vec<String> {
    let config = fs::read_to_string(workspace().join(".config/nextest.toml"))
        .unwrap_or_else(|error| panic!("the nextest config: {error}"));
    config
        .split("[[profile.default.overrides]]")
        .skip(1)
        .filter(|block| {
            GROUPS
                .iter()
                .any(|group| block.contains(&format!("test-group = '{group}'")))
        })
        .filter_map(|block| {
            let start = block.find("filter = '")? + "filter = '".len();
            let end = start + block[start..].find('\'')?;
            Some(block[start..end].to_owned())
        })
        .collect()
}

fn test_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | ".git" | "node_modules") {
                test_files(&path, found);
            }
        } else if name.ends_with(".rs") && path.components().any(|part| part.as_os_str() == "tests")
        {
            found.push(path);
        }
    }
}

/// The nearest `Cargo.toml` with a `[package]` above `file`.
fn package_of(file: &Path) -> String {
    let mut dir = file.parent();
    while let Some(at) = dir {
        if let Ok(manifest) = fs::read_to_string(at.join("Cargo.toml"))
            && let Some(section) = manifest.split("[package]").nth(1)
            && let Some(line) = section
                .lines()
                .find(|line| line.trim_start().starts_with("name"))
        {
            return line.split('"').nth(1).unwrap_or_default().to_owned();
        }
        dir = at.parent();
    }
    panic!("{} is in no package", file.display())
}

fn whole(filter: &str, package: &str) -> bool {
    let named = format!("package({package})");
    filter.contains(&format!("{named} or"))
        || filter.trim_end().ends_with(&named)
        || filter.trim() == named
}

#[test]
fn every_browser_test_is_bounded() {
    let filters = filters();
    assert!(!filters.is_empty(), "no browser group's filter found");
    let mut files = Vec::new();
    test_files(&workspace(), &mut files);
    let unbounded: Vec<String> = files
        .iter()
        .filter(|file| fs::read_to_string(file).is_ok_and(|text| text.contains(TAKES)))
        .filter(|file| {
            let package = package_of(file);
            let stem = file
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let module = stem == "mod" || file.parent().is_some_and(|dir| !dir.ends_with("tests"));
            !filters.iter().any(|filter| {
                whole(filter, &package)
                    || (!module
                        && filter.contains(&format!("package({package})"))
                        && filter.contains(&format!("binary({stem})")))
            })
        })
        .map(|file| {
            file.strip_prefix(workspace())
                .unwrap_or(file)
                .display()
                .to_string()
        })
        .collect();
    assert!(
        unbounded.is_empty(),
        "these drive a browser but no browser group in .config/nextest.toml names them: {unbounded:?}"
    );
}
