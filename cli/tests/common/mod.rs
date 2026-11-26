//! What the integration tests that build a *generated* project share.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GENERATED: &str = "generated-builds";

/// Runs `cargo <args>` in the generated project at `project`, in a build directory of its own.
///
/// An inherited `CARGO_TARGET_DIR` would put every generated workspace in one directory,
/// each build invalidating the last.
///
/// # Panics
/// If cargo cannot be spawned.
pub fn cargo_in_generated(project: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO"))
        .args(args)
        .current_dir(project)
        .env("CARGO_TARGET_DIR", generated_build_dir(project))
        .output()
        .unwrap_or_else(|error| panic!("spawn cargo {args:?} in {}: {error}", project.display()))
}

/// Nested inside the `target/` in play, so `cargo clean` still reaches it.
pub fn generated_build_dir(project: &Path) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_DIR").map_or_else(workspace_target, PathBuf::from);
    let name = project.file_name().unwrap_or_else(|| {
        panic!(
            "a generated project has a directory name: {}",
            project.display()
        )
    });
    base.join(GENERATED).join(name)
}

fn workspace_target() -> PathBuf {
    // From the manifest, not the working directory a test may have left.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli package sits inside the workspace root")
        .join("target")
}

/// The project's `fun.toml` at `root`, as text.
///
/// # Panics
/// If it cannot be read.
#[allow(dead_code)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
pub fn manifest_text(root: &Path) -> String {
    let file = root.join("fun.toml");
    std::fs::read_to_string(&file)
        .unwrap_or_else(|error| panic!("read {}: {error}", file.display()))
}
