//! Every package in the workspace declares `publish = false`.
//!
//! A release is a signed tag built by path, and cargo-deny's `allow-wildcard-paths` covers only unpublishable crates. Cargo is asked, so inherited `publish` is judged resolved.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[test]
fn every_package_is_unpublishable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<name> sits two levels under the workspace root");
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(root)
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("cargo metadata is JSON");
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata lists packages");

    // An empty list would pass every assertion below.
    assert!(
        packages.len() > 50,
        "cargo metadata listed only {} packages, so this test has stopped reading the workspace",
        packages.len()
    );

    // `publish = false` is reported as an empty list of registries; `null` means any.
    let publishable: Vec<&str> = packages
        .iter()
        .filter(|package| {
            package["publish"]
                .as_array()
                .is_none_or(|to| !to.is_empty())
        })
        .filter_map(|package| package["name"].as_str())
        .collect();
    assert!(
        publishable.is_empty(),
        "these packages may be published to a registry, and the framework never is: \
         {publishable:?}. Give each `publish.workspace = true` (or `publish = false` where it \
         does not inherit the workspace package)."
    );
}
