//! An app that migrates YSQL alone links no CQL code, and no build links `rustls-pemfile`.
//!
//! Asked of cargo's own resolution: the umbrella's feature forwarding decides it.

use std::path::Path;
use std::process::Command;

/// The workspace root this crate sits in.
fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/database_migrator is two levels below the workspace root")
}

/// The crates `fundamental` links as a normal dependency with exactly `features`.
fn linked_by(features: &str) -> String {
    let output = Command::new(env!("CARGO"))
        .current_dir(workspace())
        .args([
            "tree",
            "--offline",
            "--package",
            "fundamental",
            "--no-default-features",
            "--features",
            features,
            "--edges",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn the_migrator_with_ysql_alone_links_no_cql_crate() {
    let linked = linked_by("migrator,sql");
    assert!(linked.contains("fundamental_db_migrator"), "{linked}");
    assert!(linked.contains("fundamental_sql"), "{linked}");
    for absent in ["fundamental_cql", "scylla", "rustls-pemfile"] {
        assert!(
            !linked.lines().any(|line| line.starts_with(absent)),
            "a YSQL-only migrator links {absent}"
        );
    }
}

#[test]
fn the_migrator_with_ycql_links_the_cql_crate() {
    let linked = linked_by("migrator,cql");
    assert!(
        linked
            .lines()
            .any(|line| line.starts_with("fundamental_cql")),
        "{linked}"
    );
    assert!(
        !linked
            .lines()
            .any(|line| line.starts_with("fundamental_sql ")),
        "{linked}"
    );
}

#[test]
fn no_build_of_this_workspace_links_rustls_pemfile() {
    let lock = std::fs::read_to_string(workspace().join("Cargo.lock")).expect("a lockfile");
    assert!(
        !lock.contains("name = \"rustls-pemfile\""),
        "rustls-pemfile is unmaintained (RUSTSEC-2025-0134); parse PEM with rustls-pki-types"
    );
}
