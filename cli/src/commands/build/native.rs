//! The native-surface builder, shared by `fun build` and `fun dev`: webview bundle and host crate.

use std::path::Path;
use std::process::Command;

use clap::ValueEnum;

use crate::helpers::Project;
use crate::ui::Ui;
use crate::{CliError, CliResult};

use super::web::build_bundle_into;

/// Where a native app runs or packages: the host desktop OS, iOS, or Android
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum NativePlatform {
    /// The host desktop OS (plain cargo, no extra tooling)
    Desktop,
    /// iOS via the Tauri CLI
    Ios,
    /// Android via the Tauri CLI
    Android,
}

impl NativePlatform {
    pub(crate) fn tauri_name(self) -> Option<&'static str> {
        match self {
            Self::Desktop => None,
            Self::Ios => Some("ios"),
            Self::Android => Some("android"),
        }
    }
}

/// Build the `app`'s webview bundle into `apps/<app>/native/dist`, with its stylesheet.
///
/// # Errors
/// As [`build_bundle_into`], plus [`CliError::Io`] if the stylesheet copy fails.
pub(crate) fn build_native_bundle(
    project: &Project,
    app: &str,
    release: bool,
    css_command: Option<&str>,
    ui: &Ui,
) -> CliResult<()> {
    let out_dir = native_dir(project, app).join("dist");
    build_bundle_into(
        project,
        app,
        release,
        css_command,
        ui,
        "tauri",
        &out_dir,
        &[],
    )?;

    if css_command.is_some() {
        let css = project.app_dir(app).join("dist").join("app.css");
        if css.exists() {
            std::fs::copy(&css, out_dir.join("app.css")).map_err(|e| CliError::Io {
                path: css.display().to_string(),
                reason: format!("could not copy the stylesheet into the native dist: {e}"),
            })?;
        }
    }
    Ok(())
}

pub(crate) fn native_dir(project: &Project, app: &str) -> std::path::PathBuf {
    project.app_dir(app).join("native")
}

pub(crate) fn native_crate_name(app: &str) -> String {
    format!("{}_native", app.replace('-', "_"))
}

/// Compile the native host crate for desktop with `cargo build`.
///
/// # Errors
/// [`CliError::BuildFailed`] if it exits non-zero, [`CliError::Io`] if it cannot be launched.
pub(crate) fn cargo_build_native(
    project: &Project,
    app: &str,
    release: bool,
    ui: &Ui,
) -> CliResult<()> {
    let crate_name = native_crate_name(app);
    ui.section(format!("Building {app}'s native host"));
    ui.detail(format!(
        "profile: {}",
        if release { "release" } else { "debug" }
    ));
    ui.note("desktop installers: `fun build <app> --bundle`; mobile artifacts: `fun build <app> --platform ios|android`");

    let mut cargo = Command::new("cargo");
    cargo
        .arg("build")
        .arg("-p")
        .arg(&crate_name)
        .current_dir(project.root());
    if release {
        cargo.arg("--release");
    }
    let status = cargo.status().map_err(|e| CliError::Io {
        path: "cargo".to_string(),
        reason: format!("could not run cargo build: {e}"),
    })?;
    if !status.success() {
        return Err(CliError::BuildFailed {
            code: status.code().unwrap_or(-1),
        });
    }
    Ok(())
}

/// Package through `cargo tauri build` or `cargo tauri ios|android build`, initialising `gen/` first.
///
/// # Errors
/// [`CliError::TauriCliMissing`], [`CliError::BuildFailed`] or [`CliError::Io`].
pub(crate) fn cargo_tauri_build(
    project: &Project,
    app: &str,
    platform: NativePlatform,
    release: bool,
    ui: &Ui,
) -> CliResult<()> {
    ensure_tauri_cli()?;
    let native_dir = native_dir(project, app);

    let mut args: Vec<&str> = vec!["tauri"];
    match platform.tauri_name() {
        Some(mobile) => {
            ensure_mobile_project(&native_dir, mobile, ui)?;
            args.push(mobile);
            ui.section(format!("Packaging {app} for {mobile}"));
        }
        None => ui.section(format!("Packaging {app} desktop installers")),
    }
    args.push("build");
    if !release {
        args.push("--debug");
    }
    ui.detail(format!(
        "profile: {}",
        if release { "release" } else { "debug" }
    ));

    let status = Command::new("cargo")
        .args(&args)
        .current_dir(&native_dir)
        .status()
        .map_err(|e| CliError::Io {
            path: "cargo tauri".to_string(),
            reason: format!("could not run cargo tauri build: {e}"),
        })?;
    if !status.success() {
        return Err(CliError::BuildFailed {
            code: status.code().unwrap_or(-1),
        });
    }
    Ok(())
}

/// Ensure the Tauri CLI is installed (`cargo tauri`).
///
/// # Errors
/// [`CliError::TauriCliMissing`] with the install command when absent.
pub(crate) fn ensure_tauri_cli() -> CliResult<()> {
    let available = Command::new("cargo")
        .args(["tauri", "--version"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if available {
        Ok(())
    } else {
        Err(CliError::TauriCliMissing)
    }
}

/// Run `cargo tauri <platform> init` once, when `gen/` is absent.
///
/// # Errors
/// [`CliError::BuildFailed`] if the init exits non-zero, [`CliError::Io`] if it cannot be launched.
pub(crate) fn ensure_mobile_project(native_dir: &Path, platform: &str, ui: &Ui) -> CliResult<()> {
    let gen_dir = match platform {
        "ios" => native_dir.join("gen").join("apple"),
        _ => native_dir.join("gen").join("android"),
    };
    if gen_dir.exists() {
        return Ok(());
    }
    ui.section(format!("Initializing the {platform} project (gen/)"));
    ui.note("commit gen/ afterwards — signing config and plist/gradle edits live there");
    let status = Command::new("cargo")
        .args(["tauri", platform, "init"])
        .current_dir(native_dir)
        .status()
        .map_err(|e| CliError::Io {
            path: "cargo tauri".to_string(),
            reason: format!("could not run cargo tauri {platform} init: {e}"),
        })?;
    if !status.success() {
        return Err(CliError::BuildFailed {
            code: status.code().unwrap_or(-1),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::native_crate_name;

    #[test]
    fn native_crate_name_underscores_the_app() {
        assert_eq!(native_crate_name("main"), "main_native");
        assert_eq!(native_crate_name("point-of-sale"), "point_of_sale_native");
    }
}
