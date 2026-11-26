//! A test binary that starts a database container or a browser runs only with `--profile release`.
//!
//! A container or browser in the `default` and `agent` profiles turns a seconds loop into minutes, so such binaries are listed by `binary_id` in `.config/nextest.toml`.

use std::fs;
use std::path::{Path, PathBuf};

/// What a test binary that starts a container or a browser says.
const SLOW: &[&str] = &[
    "fundamental_test_containers",
    "test_containers::",
    "SqlSchema::mint",
    "CqlKeyspace",
    "fundamental_test_browser",
    "chromiumoxide",
    "start_container",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root")
        .to_path_buf()
}

fn packages(root: &Path, found: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if !path.is_dir() || name == "target" || name.to_string_lossy().starts_with('.') {
            continue;
        }
        let manifest = path.join("Cargo.toml");
        if path.join("tests").is_dir()
            && let Ok(text) = fs::read_to_string(&manifest)
            && let Some(package) = text
                .split("[package]")
                .nth(1)
                .and_then(|section| {
                    section
                        .lines()
                        .find(|line| line.trim_start().starts_with("name"))
                })
                .and_then(|line| line.split('"').nth(1))
        {
            found.push((package.to_owned(), path.clone()));
        }
        packages(&path, found);
    }
}

/// The text of a test binary and of the modules it declares beside it.
fn binary_text(tests: &Path, file: &Path) -> String {
    let text = fs::read_to_string(file).unwrap_or_default();
    let mut all = text.clone();
    for line in text.lines() {
        let line = line.trim();
        let Some(module) = line
            .strip_prefix("mod ")
            .or_else(|| line.strip_prefix("pub mod "))
            .and_then(|rest| rest.strip_suffix(';'))
        else {
            continue;
        };
        for candidate in [
            tests.join(module).join("mod.rs"),
            tests.join(format!("{module}.rs")),
        ] {
            all.push_str(&fs::read_to_string(candidate).unwrap_or_default());
        }
    }
    all
}

fn tree_text(dir: &Path) -> String {
    let mut all = String::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                all.push_str(&tree_text(&path));
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                all.push_str(&fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }
    all
}

#[test]
fn every_test_that_starts_a_container_or_a_browser_waits_for_release() {
    let root = workspace();
    let config = fs::read_to_string(root.join(".config/nextest.toml")).expect("the nextest config");
    let filters: Vec<&str> = ["[profile.default]", "[profile.agent]"]
        .iter()
        .map(|profile| {
            let at = config.find(profile).expect("the profile");
            config[at..]
                .lines()
                .find(|line| line.starts_with("default-filter"))
                .expect("its filter")
        })
        .collect();
    let mut found = Vec::new();
    packages(&root, &mut found);
    let mut missing = Vec::new();
    for (package, dir) in found {
        let tests = dir.join("tests");
        let Ok(entries) = fs::read_dir(&tests) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let (binary, text) = if path.extension().is_some_and(|extension| extension == "rs") {
                let stem = path
                    .file_stem()
                    .expect("a name")
                    .to_string_lossy()
                    .into_owned();
                (stem, binary_text(&tests, &path))
            } else if path.join("main.rs").exists() {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    tree_text(&path),
                )
            } else {
                continue;
            };
            // This file names the markers it looks for.
            let itself = binary == "every_slow_test_waits_for_release";
            let slow = !itself && SLOW.iter().any(|marker| text.contains(marker));
            let id = format!("binary_id({package}::{binary})");
            let waits = filters.iter().all(|filter| {
                filter.contains(&id) || filter.contains(&format!("package({package})"))
            });
            if slow && !waits {
                missing.push(id);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "these test binaries start a container or a browser; add them to the default and agent \
         filters in .config/nextest.toml:\n{}",
        missing.join("\n")
    );
}
