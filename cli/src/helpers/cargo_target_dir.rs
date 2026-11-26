use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{debug, warn};

use crate::CliError;

/// The directory cargo builds the workspace at `root` into, as `cargo metadata` resolves it.
///
/// # Errors
/// [`CliError::CommandSpawn`] when `cargo metadata` fails or answers no `target_directory`.
pub(crate) fn cargo_target_dir(root: &Path) -> Result<PathBuf, CliError> {
    target_directory_from(&mut metadata_command(root))
}

fn metadata_command(root: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .current_dir(root);
    command
}

fn target_directory_from(command: &mut Command) -> Result<PathBuf, CliError> {
    let failed = |reason: String| {
        warn!(%reason, "cargo metadata could not say where the build goes");
        CliError::CommandSpawn {
            bin: "cargo metadata".to_owned(),
            reason,
        }
    };
    let output = command
        .output()
        .map_err(|error| failed(error.to_string()))?;
    if !output.status.success() {
        return Err(failed(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| failed(error.to_string()))?;
    let target = metadata
        .get("target_directory")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| failed("no target_directory in its answer".to_owned()))?;
    debug!(target = %target.display(), "the workspace builds here");
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::{metadata_command, target_directory_from};
    use std::path::Path;

    fn framework_root() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
    }

    #[test]
    fn a_redirected_target_is_where_the_build_goes() {
        let redirected = std::env::temp_dir().join("fundamental-cargo-target-dir-probe");
        let mut command = metadata_command(framework_root());
        command.env("CARGO_TARGET_DIR", &redirected);
        assert_eq!(
            target_directory_from(&mut command).expect("cargo metadata answers"),
            redirected
        );
    }

    #[test]
    fn a_relative_redirect_resolves_against_the_workspace() {
        let mut command = metadata_command(framework_root());
        command.env("CARGO_TARGET_DIR", "elsewhere/target");
        let target = target_directory_from(&mut command).expect("cargo metadata answers");
        assert!(target.is_absolute(), "{}", target.display());
        assert!(target.ends_with("elsewhere/target"), "{}", target.display());
    }

    #[test]
    fn a_workspace_that_does_not_exist_is_an_error() {
        let error = target_directory_from(&mut metadata_command(Path::new(
            "/nonexistent-workspace-for-cargo-target-dir",
        )))
        .expect_err("no manifest, no answer");
        assert!(matches!(error, crate::CliError::CommandSpawn { .. }));
    }
}
