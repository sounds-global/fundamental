//! A server links no OS keychain; a desktop app does.
//!
//! Asked of cargo's resolution, because the umbrella's feature forwarding decides it.

use std::path::Path;
use std::process::Command;

fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/keys is two levels below the workspace root")
}

/// Whether `fundamental` with exactly `features` links the `keyring` crate.
fn links_keyring(features: &str) -> bool {
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
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.starts_with("keyring "))
}

#[test]
fn a_server_with_sign_in_links_no_keychain() {
    assert!(!links_keyring("auth,keys,server,web"));
}

#[test]
fn a_desktop_app_links_the_keychain() {
    assert!(links_keyring("native"));
}

#[test]
fn the_keychain_is_asked_for_by_name() {
    assert!(links_keyring("keys-os-keychain"));
}
