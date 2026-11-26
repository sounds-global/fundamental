//! Launches an app's `migrate` binary via `cargo run`, where its models and connections exist.

use std::process::Command;

use crate::helpers::{Project, resolve_app_bin};
use crate::{CliError, CliResult, CommandContext};

const ROLE: &str = "migrate";

/// Runs `cargo run [-p <app>] --bin <bin> -- <words…>`, each word one argument, never through a shell.
///
/// # Errors
/// [`CliError::MigrateSpawn`], [`CliError::MigrateFailed`] or [`CliError::AppBinMissing`].
pub(crate) fn run(
    app: Option<&str>,
    bin: Option<&str>,
    words: &[&str],
    ctx: &CommandContext,
) -> CliResult<()> {
    let project = Project::discover().ok();
    let package = resolve_package(app, project.as_ref());
    let bin = resolve_bin(bin, package.as_deref(), project.as_ref())?;

    let mut cargo_args = vec!["run".to_string()];
    if let Some(package) = &package {
        cargo_args.push("-p".to_string());
        cargo_args.push(package.clone());
    }
    cargo_args.push("--bin".to_string());
    cargo_args.push(bin.clone());
    cargo_args.push("--".to_string());
    cargo_args.extend(words.iter().map(|word| (*word).to_string()));

    let ui = ctx.ui();
    ui.section(format!(
        "Migrate ({})",
        words.first().copied().unwrap_or_default()
    ));
    ui.detail(format!("cargo {}", cargo_args.join(" ")));

    let status = Command::new("cargo")
        .args(&cargo_args)
        .status()
        .map_err(|err| CliError::MigrateSpawn {
            bin: bin.clone(),
            reason: err.to_string(),
        })?;

    if !status.success() {
        return Err(CliError::MigrateFailed {
            code: status.code().unwrap_or(1),
        });
    }
    Ok(())
}

/// `--app`, else `main` inside a project, else `None` outside one.
fn resolve_package(app: Option<&str>, project: Option<&Project>) -> Option<String> {
    if let Some(app) = app {
        return Some(app.to_string());
    }
    project.map(|_| "main".to_string())
}

/// `--bin`, else the app's manifest; outside a project, the bare role.
fn resolve_bin(
    bin: Option<&str>,
    package: Option<&str>,
    project: Option<&Project>,
) -> CliResult<String> {
    if let Some(bin) = bin {
        return Ok(bin.to_string());
    }
    match (project, package) {
        (Some(project), Some(package)) => resolve_app_bin(project, package, ROLE),
        _ => Ok(ROLE.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn project_with(app: &str, bins: &[&str]) -> (TempDir, Project) {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("fun.toml"), "[project]\nname = \"p\"\n").expect("manifest");
        let app_dir = tmp.path().join("apps").join(app);
        fs::create_dir_all(&app_dir).expect("app dir");
        let body: String = bins
            .iter()
            .map(|b| format!("[[bin]]\nname = \"{b}\"\n\n"))
            .collect();
        fs::write(app_dir.join("Cargo.toml"), body).expect("crate manifest");
        let project = Project::discover_from(tmp.path()).expect("discoverable");
        (tmp, project)
    }

    #[test]
    fn an_explicit_bin_wins_over_the_convention() {
        let (_t, project) = project_with("layer0", &["layer0-migrate"]);
        let bin = resolve_bin(Some("ops"), Some("layer0"), Some(&project)).expect("explicit");
        assert_eq!(bin, "ops");
    }

    #[test]
    fn the_convention_resolves_the_prefixed_binary() {
        let (_t, project) = project_with("layer0", &["layer0", "layer0-migrate"]);
        let bin = resolve_bin(None, Some("layer0"), Some(&project)).expect("resolved");
        assert_eq!(bin, "layer0-migrate");
    }

    #[test]
    fn outside_a_project_the_bare_role_stands() {
        let bin = resolve_bin(None, None, None).expect("bare role");
        assert_eq!(bin, "migrate");
    }

    #[test]
    fn an_app_without_a_migration_binary_is_an_error() {
        use fundamental::core::Error as _;
        let (_t, project) = project_with("layer0", &["layer0"]);
        let err = resolve_bin(None, Some("layer0"), Some(&project)).expect_err("no migrator");
        assert_eq!(err.error_code(), "CLI_041");
    }

    #[test]
    fn an_explicit_app_selects_the_package() {
        let (_t, project) = project_with("layer0", &["layer0-migrate"]);
        assert_eq!(
            resolve_package(Some("layer0"), Some(&project)),
            Some("layer0".to_string())
        );
        assert_eq!(
            resolve_package(None, Some(&project)),
            Some("main".to_string())
        );
        assert_eq!(resolve_package(None, None), None);
    }
}
