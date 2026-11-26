//! `fun seed`: gate the environment, then launch the seeder via `cargo run [-p <app>] --bin <name>`.

use std::process::Command;

use fundamental::config::{AppSettings, ResolvedEnvironment};
use fundamental::core::BuildProfile;
use fundamental::core::env::EnvVars;
use fundamental::seed::{RELEASE_OVERRIDE_VAR, SeedError, guard_environment};

use super::SeedArgs;
use crate::helpers::Project;
use crate::{CliError, CliResult, CommandContext};

/// Gate the environment, resolve the app whose seeds to run, and launch its seeder.
pub(crate) fn run(args: SeedArgs, ctx: CommandContext) -> CliResult<()> {
    // A fast pre-check before any compile; the seeder re-checks in-process with the same resolver.
    let environment = ResolvedEnvironment::for_manifest(
        &EnvVars::process(),
        AppSettings::MANIFEST,
        BuildProfile::Debug,
    )
    .map_err(SeedError::from)?;
    guard_environment(&environment, args.release)?;

    let package = resolve_package(args.app.as_deref())?;
    let cargo_args = cargo_args(package.as_deref(), &args.bin);

    let ui = ctx.ui();
    ui.section(format!("Seeding ({})", environment.name()));
    ui.detail(format!("cargo {}", cargo_args.join(" ")));

    let status = Command::new("cargo")
        .args(&cargo_args)
        .env(RELEASE_OVERRIDE_VAR, override_value(args.release))
        .status()
        .map_err(|err| CliError::SeedSpawn {
            bin: args.bin.clone(),
            reason: err.to_string(),
        })?;

    if !status.success() {
        return Err(CliError::SeedFailed {
            code: status.code().unwrap_or(1),
        });
    }

    Ok(())
}

/// The app owning the seeds: `--app`, the sole app with `seeds/`, or `None` for a package-less run.
///
/// # Errors
/// [`CliError::AmbiguousSeedApp`] when several apps define seeds and no `--app` was given.
fn resolve_package(app: Option<&str>) -> Result<Option<String>, CliError> {
    if let Some(app) = app {
        return Ok(Some(app.to_string()));
    }
    let Ok(project) = Project::discover() else {
        return Ok(None);
    };
    let mut apps = project.apps_with_seeds();
    match apps.len() {
        0 => Ok(None),
        1 => Ok(Some(apps.remove(0))),
        _ => Err(CliError::AmbiguousSeedApp {
            apps: apps.join(", "),
        }),
    }
}

fn cargo_args(package: Option<&str>, bin: &str) -> Vec<String> {
    let mut args = vec!["run".to_string()];
    if let Some(package) = package {
        args.push("-p".to_string());
        args.push(package.to_string());
    }
    args.push("--bin".to_string());
    args.push(bin.to_string());
    args
}

/// Set explicitly either way, so an inherited value cannot re-enable the override.
fn override_value(release: bool) -> &'static str {
    if release { "1" } else { "0" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_value_reflects_the_flag() {
        assert_eq!(override_value(true), "1");
        assert_eq!(override_value(false), "0");
    }

    #[test]
    fn cargo_args_without_a_package_omit_dash_p() {
        assert_eq!(cargo_args(None, "seed"), vec!["run", "--bin", "seed"]);
    }

    #[test]
    fn cargo_args_with_a_package_target_it() {
        assert_eq!(
            cargo_args(Some("main"), "seed"),
            vec!["run", "-p", "main", "--bin", "seed"]
        );
    }
}
