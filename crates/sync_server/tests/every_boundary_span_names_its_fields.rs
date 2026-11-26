//! Every span at the auth and sync boundaries names its fields; none records its arguments.
//!
//! `#[instrument]` records every argument not skipped, so each boundary span must `skip_all`.

use std::fs;
use std::path::{Path, PathBuf};

/// The source a token or a sync frame reaches before any app code.
const BOUNDARY: &[&str] = &[
    "../auth/src",
    "../ws/src",
    "src",
    "../sync/src/wire",
    "../sync/src/change",
    "../cql/src/slice_store.rs",
];

fn rust_files(path: &Path, into: &mut Vec<PathBuf>) {
    if path.is_file() {
        into.push(path.to_path_buf());
        return;
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            rust_files(&entry, into);
        } else if entry.extension().is_some_and(|ext| ext == "rs") {
            into.push(entry);
        }
    }
}

#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn instrument_attributes(source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut rest = source;
    let mut offset = 0;
    while let Some(start) = rest.find("#[instrument") {
        let attribute = &rest[start..];
        let mut depth = 0usize;
        let mut end = attribute.len();
        for (index, ch) in attribute.char_indices() {
            match ch {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = index + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let line = source[..offset + start].lines().count() + 1;
        found.push((
            line,
            attribute[..end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        ));
        offset += start + end;
        rest = &rest[start + end..];
    }
    found
}

#[test]
fn every_boundary_span_names_its_fields() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for path in BOUNDARY {
        rust_files(&root.join(path), &mut files);
    }
    let mut seen = 0;
    let mut offenders = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).expect("readable source");
        for (line, attribute) in instrument_attributes(&source) {
            seen += 1;
            if !attribute.contains("skip_all") {
                offenders.push(format!("{}:{line}: {attribute}", file.display()));
            }
        }
    }
    assert!(
        seen >= 10,
        "the scan found {seen} spans; it is not reading the boundary"
    );
    assert!(
        offenders.is_empty(),
        "a boundary span records its arguments; give it `skip_all` and name its fields:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn a_span_that_records_its_arguments_is_found() {
    let source = "fn a() {}\n#[instrument(skip(self), fields(slice = %name))]\nasync fn b() {}\n";
    let found = instrument_attributes(source);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, 2);
    assert!(!found[0].1.contains("skip_all"));
}
