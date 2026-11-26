//! A release's check loads it under the bootstrap subscriber, run as a child process.

use std::env;
use std::path::Path;
use std::process::Command;

/// Set on the child: the manifest of the release it checks.
const CHILD: &str = "FUNDAMENTAL_RELEASE_CHECK_CHILD";

/// The child's half; a no-op unless spawned as a child below.
#[test]
fn child_checks_a_release() {
    let Ok(manifest) = env::var(CHILD) else {
        return;
    };
    match fundamental_observability::check_release("main", &manifest) {
        Ok(app) => println!("checked {}", app.name()),
        Err(error) => println!("refused: {error}"),
    }
}

/// A deployed release's manifest with `observability` as its `[observability]` section.
fn release(dir: &Path, observability: &str) -> String {
    let manifest = dir.join("fun.toml");
    std::fs::write(
        &manifest,
        format!(
            "[apps.main]\nweb = {{ port = 9000, tls = \"terminated_upstream\" }}\n\n\
             [observability]\n{observability}\n\n[observability.otlp]\nenabled = false\n"
        ),
    )
    .expect("the release's manifest");
    manifest.display().to_string()
}

fn checked(manifest: &str) -> String {
    let binary = env::current_exe().expect("the test binary knows its path");
    let output = Command::new(binary)
        .args(["--exact", "child_checks_a_release", "--nocapture"])
        .env(CHILD, manifest)
        .env("FUNDAMENTAL_ENV", "production")
        .env_remove("RUST_LOG")
        .output()
        .expect("the child runs");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_release_is_loaded_with_a_subscriber_listening() {
    let dir = tempfile::tempdir().expect("a release directory");

    let said = checked(&release(dir.path(), "log_level = \"warn\""));

    assert!(said.contains("checked main"), "{said}");
    assert!(said.contains("app config loaded"), "{said}");
}

#[test]
fn a_release_whose_telemetry_would_not_install_is_refused_naming_it() {
    let dir = tempfile::tempdir().expect("a release directory");

    let said = checked(&release(
        dir.path(),
        "log_level = \"warn,fundamental=loud\"",
    ));

    assert!(said.contains("refused:"), "{said}");
    assert!(said.contains("fundamental=loud"), "{said}");
}
