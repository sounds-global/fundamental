//! Every test that builds a generated project is bounded by the nextest group, and builds where it should.
//!
//! A nextest filter matches only names, and an inherited `CARGO_TARGET_DIR` makes generated
//! workspaces share one lock, so both rules are checked here by name.

use std::fs;
use std::path::Path;

/// Keep in step with `.config/nextest.toml`.
const BOUNDED_SPELLINGS: [&str; 2] = ["compiles", "builds"];

const BOUNDED_BINARIES: [&str; 2] = ["generate", "new"];

/// This file, which holds every marker below as a fixture rather than as a call.
const THIS_CHECKER: &str = "every_cargo_build_is_bounded";

/// `assert_cargo_check` goes through `cargo_in_generated`; `run_in` with `build` may not compile.
const REACHES_A_BUILD: [&str; 3] = [
    "cargo_in_generated(",
    "assert_cargo_check(",
    "run_in(&root, &[\"build\"",
];

/// A test driving `fun` must be offline: a generated project's git-over-SSH dependency would be fetched.
const DRIVES_THE_CLI: &str = "CARGO_BIN_EXE_fun";

const MUST_BE_OFFLINE: &str = "CARGO_NET_OFFLINE";

/// A build without the helper inherits the run's `CARGO_TARGET_DIR`.
const BYPASSES_THE_HELPER: [&str; 2] = ["Command::new(\"cargo\")", "Command::new(env!(\"CARGO\"))"];

/// Refused before anything compiles: behaviour at runtime, which reading the source cannot decide.
const REFUSES_BEFORE_COMPILING: [(&str, &str); 1] = [(
    "build_platform_flags_require_the_native_surface",
    "asserts `fun build --platform ios` is refused on a project with no native surface, \
     so it never reaches a compile",
)];

#[test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn every_test_that_builds_a_generated_project_is_bounded_and_builds_in_its_own_directory() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut unbounded: Vec<String> = Vec::new();
    let mut bypassing: Vec<String> = Vec::new();
    let mut online: Vec<String> = Vec::new();
    let mut found = 0usize;

    for entry in fs::read_dir(&tests).expect("read cli/tests") {
        let path = entry.expect("a directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("a file stem")
            .to_owned();
        if stem == THIS_CHECKER {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read a test file");

        if source.contains(DRIVES_THE_CLI) && !source.contains(MUST_BE_OFFLINE) {
            online.push(stem.clone());
        }

        for (name, body) in test_functions(&source) {
            let bypasses = BYPASSES_THE_HELPER
                .iter()
                .any(|marker| body.contains(marker));
            let reaches = bypasses || REACHES_A_BUILD.iter().any(|m| body.contains(m));
            if !reaches
                || REFUSES_BEFORE_COMPILING
                    .iter()
                    .any(|(allowed, _)| *allowed == name)
            {
                continue;
            }
            found += 1;
            if bypasses {
                bypassing.push(format!("  {stem}.rs::{name}"));
            }
            let named_for_the_group = BOUNDED_SPELLINGS.iter().any(|s| name.contains(s));
            let in_a_reachable_binary = BOUNDED_BINARIES.contains(&stem.as_str());
            if !named_for_the_group || !in_a_reachable_binary {
                unbounded.push(format!(
                    "  {stem}.rs::{name} — {}",
                    if in_a_reachable_binary {
                        format!("name carries none of {BOUNDED_SPELLINGS:?}")
                    } else {
                        format!("`{stem}` is not one of the binaries {BOUNDED_BINARIES:?}")
                    }
                ));
            }
        }
    }

    assert!(
        online.is_empty(),
        "these files drive the `fun` binary without pinning `{MUST_BE_OFFLINE}` on the \
         invocation: {online:?}\n\nA `fun` verb that ends in cargo will resolve the generated \
         project's dependencies, and `fundamental` comes from git over SSH — so the test \
         reaches the network, prompts for a key, and fails outright on a machine that has \
         none. Set `.env(\"{MUST_BE_OFFLINE}\", \"true\")` on the helper that spawns `fun`; \
         the real compiles go through `common::cargo_in_generated`, which is online on \
         purpose because its git dependencies are rewritten to local paths first."
    );

    assert!(
        bypassing.is_empty(),
        "these tests spawn cargo themselves, so the build inherits the run's \
         `CARGO_TARGET_DIR` and lands in a directory shared with every other generated \
         project — one exclusive lock, and each build invalidating the last:\n{}\n\nUse \
         `common::cargo_in_generated`, which gives the project a build directory of its own.",
        bypassing.join("\n")
    );

    assert!(
        unbounded.is_empty(),
        "these tests reach a build and the `cargo-builds` group cannot see them, so they \
         will race the ones it does — which is a 300 s timeout under a full run, on a \
         different test each time:\n{}\n\nEither rename them, widen the filter in \
         `.config/nextest.toml` and the constants here together, or add an entry to \
         `REFUSES_BEFORE_COMPILING` with the reason it reaches no compile.",
        unbounded.join("\n")
    );

    // A scanner that matched nothing would pass both assertions for the wrong reason.
    assert!(
        found >= 6,
        "expected at least the six build-reaching tests this bound was written for, \
         found {found} — the scanner has stopped seeing them"
    );
}

/// Every `#[test] fn` in `source`, as `(name, body)`, at any indentation (formatted code assumed).
fn test_functions(source: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut lines = source.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() != "#[test]" {
            continue;
        }
        let Some(signature) = lines.next() else { break };
        let indent = " ".repeat(signature.len() - signature.trim_start().len());
        let Some(name) = signature
            .trim_start()
            .strip_prefix("fn ")
            .and_then(|rest| rest.split('(').next())
        else {
            continue;
        };
        let closing = format!("{indent}}}");
        let mut body = String::new();
        for line in lines.by_ref() {
            if line == closing {
                break;
            }
            body.push_str(line);
            body.push('\n');
        }
        found.push((name.to_owned(), body));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scanner_reads_a_name_and_its_body() {
        let source = "#[test]\nfn a_named_test() {\n    let _ = 1;\n}\n";

        let found = test_functions(source);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "a_named_test");
        assert!(found[0].1.contains("let _ = 1;"));
    }

    #[test]
    fn a_test_nested_in_a_module_is_still_seen() {
        let source =
            "mod inner {\n    #[test]\n    fn a_nested_test() {\n        let _ = 1;\n    }\n}\n";

        let found = test_functions(source);

        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, "a_nested_test");
        assert!(found[0].1.contains("let _ = 1;"), "{:?}", found[0].1);
    }

    #[test]
    fn a_function_without_the_attribute_is_not_a_test() {
        let source = "fn a_helper() {\n    let _ = 1;\n}\n";

        assert!(test_functions(source).is_empty());
    }
}
