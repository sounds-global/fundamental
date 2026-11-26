//! `#[non_exhaustive]` on public error enums, enforced rather than remembered.
//!
//! An exhaustive public enum turns every addition into a breaking change downstream.

use std::fs;
use std::path::{Path, PathBuf};

/// Public error enums exempt from the rule, keyed by path since names repeat; never a place to silence one.
const EXEMPT: &[(&str, &str, &str)] = &[];

#[test]
fn every_public_error_enum_declares_non_exhaustive() {
    let root = workspace_root();
    let mut found = Vec::new();
    let mut missing = Vec::new();

    for file in rust_files_under_crate_sources(&root) {
        for (name, non_exhaustive) in error_enums_in(&fs::read_to_string(&file).expect("read")) {
            found.push(name.clone());
            let shown = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .display()
                .to_string();
            let excused = EXEMPT.iter().any(|(f, n, _)| *n == name && *f == shown);
            if !non_exhaustive && !excused {
                missing.push(format!("{name} ({shown})"));
            }
        }
    }

    // Proves the scan still matches something; otherwise it would sweep over nothing.
    assert!(
        found.len() > 30,
        "the scan found only {} public error enums, which means it has stopped reading the \
         source rather than that the workspace has stopped declaring them",
        found.len()
    );

    assert!(
        missing.is_empty(),
        "these public error enums are missing `#[non_exhaustive]`, so adding a variant to any \
         of them is a breaking change for every caller that matches on it:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn an_exemption_for_an_enum_that_now_complies_is_refused() {
    let root = workspace_root();
    let compliant: Vec<&str> = EXEMPT
        .iter()
        .filter(|(path, name, _)| {
            rust_files_under_crate_sources(&root).iter().any(|file| {
                file.strip_prefix(&root)
                    .unwrap_or(file)
                    .display()
                    .to_string()
                    == **path
                    && error_enums_in(&fs::read_to_string(file).expect("read"))
                        .into_iter()
                        .any(|(found, non_exhaustive)| found == **name && non_exhaustive)
            })
        })
        .map(|(_, name, _)| *name)
        .collect();
    assert!(
        compliant.is_empty(),
        "these enums are excused and no longer need to be: {compliant:?}"
    );
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<name> sits two levels under the workspace root")
        .to_path_buf()
}

/// Every `.rs` file under `crates/*/src`; `tests/` fixtures are no public contract.
fn rust_files_under_crate_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let crates = root.join("crates");
    let Ok(entries) = fs::read_dir(&crates) else {
        return out;
    };
    for entry in entries.flatten() {
        collect_rust_files(&entry.path().join("src"), &mut out);
    }
    out.sort();
    out
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every `pub enum` deriving `Error` and whether it is `#[non_exhaustive]`, gathered by paren balance since attributes wrap.
fn error_enums_in(source: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut attrs: Vec<String> = Vec::new();
    let mut open = String::new();
    let mut depth = 0_isize;

    for line in source.lines() {
        let trimmed = line.trim();

        if depth > 0 {
            open.push(' ');
            open.push_str(trimmed);
            depth += balance(trimmed);
            if depth <= 0 {
                attrs.push(std::mem::take(&mut open));
                depth = 0;
            }
            continue;
        }

        if let Some(name) = trimmed.strip_prefix("pub enum ") {
            let name = name
                .split(['<', '{', ' '])
                .next()
                .unwrap_or_default()
                .to_owned();
            let derives_error = attrs.iter().any(|a| {
                a.starts_with("#[derive(")
                    && a.split(|c: char| !c.is_alphanumeric() && c != '_')
                        .any(|word| word == "Error")
            });
            if derives_error && !name.is_empty() {
                out.push((
                    name,
                    attrs.iter().any(|a| a.starts_with("#[non_exhaustive")),
                ));
            }
            attrs.clear();
            continue;
        }

        if trimmed.starts_with("#[") {
            depth = balance(trimmed);
            if depth > 0 {
                open = trimmed.to_owned();
            } else {
                attrs.push(trimmed.to_owned());
                depth = 0;
            }
            continue;
        }

        // Doc comments and blank lines may sit between the attributes and the item.
        if !trimmed.is_empty() && !trimmed.starts_with("//") {
            attrs.clear();
        }
    }
    out
}

fn balance(line: &str) -> isize {
    line.chars().fold(0, |acc, c| match c {
        '(' | '[' => acc + 1,
        ')' | ']' => acc - 1,
        _ => acc,
    })
}
