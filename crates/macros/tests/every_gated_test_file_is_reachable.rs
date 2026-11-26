//! A `tests/` file gated on a Cargo feature runs, or this fails.
//!
//! A file gated by `#![cfg(feature = "x")]` builds to an empty, green binary when `x` is off, so each such feature must be on in the crate's own manifest.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use toml::Value;

/// Test files gated on wasm, which no feature can turn on for the native run.
const ARCHITECTURE_GATED: &[&str] = &[
    "crates/keys/tests/wasm_smoke.rs",
    "crates/offline/tests/wasm_bench.rs",
    "crates/web/tests/wasm_host_sync_config.rs",
    "crates/web/tests/wasm_hydrate.rs",
    "crates/web/tests/web_action.rs",
    "crates/ws_client/tests/wasm_smoke.rs",
];

#[test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn every_feature_gated_test_file_has_that_feature_on_in_its_own_crate() {
    let root = workspace_root();
    let mut scanned = 0_usize;
    let mut unreachable = Vec::new();

    for manifest in member_manifests(&root) {
        let parsed = parse(&manifest);
        let name = package_name(&parsed);
        let activated = activated_features(&parsed, &name);

        for file in test_files_beside(&manifest) {
            let Some(gate) = file_scope_cfg(&fs::read_to_string(&file).expect("read a test file"))
            else {
                continue;
            };
            scanned += 1;
            let reachable = strip_not_groups(&gate);
            if names_wasm(&reachable) {
                continue; // architecture, not a feature — the other test owns these
            }
            for feature in features_named(&reachable) {
                if !activated.contains(&feature) {
                    unreachable.push(format!(
                        "{} is gated on `feature = \"{feature}\"`, which `{name}` does not \
                         activate for its own tests",
                        shown(&root, &file)
                    ));
                }
            }
        }
    }

    // Proves the scan still matches something; otherwise it would sweep over nothing.
    assert!(
        scanned > 30,
        "the scan found only {scanned} file-scope `#![cfg(...)]` gates across the workspace's \
         `tests/` directories, which means it has stopped reading them rather than that the \
         workspace has stopped writing them"
    );

    assert!(
        unreachable.is_empty(),
        "these test files compile to an EMPTY binary, so their tests never run and the suite \
         reports them green:\n  {}\n\nActivate each feature in that crate's own \
         `[dev-dependencies]` with `<crate> = {{ path = \".\", features = [...] }}`, or in its \
         `default` list.",
        unreachable.join("\n  ")
    );
}

#[test]
fn every_architecture_gated_test_file_is_declared() {
    let root = workspace_root();
    let mut found = BTreeSet::new();

    for manifest in member_manifests(&root) {
        for file in test_files_beside(&manifest) {
            let Some(gate) = file_scope_cfg(&fs::read_to_string(&file).expect("read a test file"))
            else {
                continue;
            };
            if names_wasm(&strip_not_groups(&gate)) {
                found.insert(shown(&root, &file));
            }
        }
    }

    let declared: BTreeSet<String> = ARCHITECTURE_GATED.iter().map(|p| (*p).to_owned()).collect();

    let undeclared: Vec<&String> = found.difference(&declared).collect();
    assert!(
        undeclared.is_empty(),
        "these test files are gated on an architecture no gate command builds, so their tests \
         never run anywhere:\n  {undeclared:#?}\n\nAdd each to ARCHITECTURE_GATED — and decide \
         how it is meant to run before you do."
    );

    let stale: Vec<&String> = declared.difference(&found).collect();
    assert!(
        stale.is_empty(),
        "ARCHITECTURE_GATED names files that are no longer architecture-gated (or no longer \
         exist): {stale:#?}. A list that excuses more than the tree contains stops describing \
         it."
    );
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<name> sits two levels under the workspace root")
        .to_path_buf()
}

fn member_manifests(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.join("cli/Cargo.toml")];
    if let Ok(entries) = fs::read_dir(root.join("crates")) {
        for entry in entries.flatten() {
            let manifest = entry.path().join("Cargo.toml");
            if manifest.is_file() {
                out.push(manifest);
            }
        }
    }
    out.retain(|m| m.is_file());
    out.sort();
    out
}

fn parse(manifest: &Path) -> Value {
    let text = fs::read_to_string(manifest).expect("read a manifest");
    toml::from_str::<Value>(&text).expect("a manifest is valid TOML")
}

fn package_name(manifest: &Value) -> String {
    manifest
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .expect("every member declares a package name")
        .to_owned()
}

/// The features a crate turns on for its own tests: `default` and the `path = "."` self dev-dependency, never another crate's.
fn activated_features(manifest: &Value, own_name: &str) -> BTreeSet<String> {
    let table: BTreeMap<String, Vec<String>> = manifest
        .get("features")
        .and_then(Value::as_table)
        .map(|t| {
            t.iter()
                .map(|(k, v)| {
                    let entries = v
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    (k.clone(), entries)
                })
                .collect()
        })
        .unwrap_or_default();

    let mut queue: Vec<String> = table.get("default").cloned().unwrap_or_default();
    // A target table whose cfg applies only to wasm is not what this gate judges.
    let native_targets = manifest
        .get("target")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
        .filter(|(cfg, _)| !names_wasm(&strip_not_groups(cfg)))
        .map(|(_, target)| target);
    for dev in std::iter::once(manifest).chain(native_targets) {
        if let Some(features) = dev
            .get("dev-dependencies")
            .and_then(|d| d.get(own_name))
            .and_then(|s| s.get("features"))
            .and_then(Value::as_array)
        {
            queue.extend(features.iter().filter_map(Value::as_str).map(str::to_owned));
        }
    }

    let mut activated = BTreeSet::new();
    while let Some(feature) = queue.pop() {
        // `dep:x`, `crate/x` and `crate?/x` name no feature of this crate.
        if feature.starts_with("dep:") || feature.contains('/') {
            continue;
        }
        if activated.insert(feature.clone())
            && let Some(implied) = table.get(&feature)
        {
            queue.extend(implied.iter().cloned());
        }
    }
    activated
}

fn test_files_beside(manifest: &Path) -> Vec<PathBuf> {
    let dir = manifest
        .parent()
        .expect("a manifest has a directory")
        .join("tests");
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The file-scope `#![cfg(...)]` attributes joined, gathered by paren balance since an attribute may wrap.
fn file_scope_cfg(source: &str) -> Option<String> {
    let mut found = String::new();
    let mut open = String::new();
    let mut depth = 0_isize;

    for line in source.lines() {
        let trimmed = line.trim();
        if depth > 0 {
            open.push(' ');
            open.push_str(trimmed);
            depth += balance(trimmed);
            if depth <= 0 {
                found.push(' ');
                found.push_str(&std::mem::take(&mut open));
                depth = 0;
            }
            continue;
        }
        if trimmed.starts_with("#![cfg(") {
            open.push_str(trimmed);
            depth = balance(trimmed);
            if depth <= 0 {
                found.push(' ');
                found.push_str(&std::mem::take(&mut open));
                depth = 0;
            }
        }
    }

    (!found.trim().is_empty()).then(|| found.trim().to_owned())
}

fn balance(text: &str) -> isize {
    text.chars().fold(0, |acc, c| match c {
        '(' => acc + 1,
        ')' => acc - 1,
        _ => acc,
    })
}

/// `gate` without its `not(...)` groups, which every native host satisfies.
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn strip_not_groups(gate: &str) -> String {
    let mut out = String::new();
    let mut rest = gate;
    while let Some(at) = rest.find("not(") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "not(".len()..];
        let mut depth = 1_isize;
        let mut end = after.len();
        for (i, c) in after.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &after[end.min(after.len())..];
    }
    out.push_str(rest);
    out
}

fn names_wasm(gate: &str) -> bool {
    gate.contains("target_arch = \"wasm32\"")
}

fn features_named(gate: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = gate;
    while let Some(at) = rest.find("feature = \"") {
        let after = &rest[at + "feature = \"".len()..];
        match after.find('"') {
            Some(end) => {
                out.push(after[..end].to_owned());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

fn shown(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string()
        .replace('\\', "/")
}
