//! `fun build`: compile an app's build artifacts.

pub(crate) mod assets;
pub(crate) mod native;
pub(crate) mod wasm;
pub(crate) mod web;

use std::process::Command;

use clap::{Args, Subcommand};

use crate::helpers::{Manifest, Project};
use crate::ui::Ui;
use crate::{CliError, CliResult, CommandContext, Surface};
use native::NativePlatform;
use wasm::WasmArgs;
use web::build_bundle;

/// Arguments for `fun build`.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct BuildArgs {
    /// The app to build (defaults to the project's sole app)
    #[arg(value_name = "APP")]
    app: Option<String>,

    /// Build with the release profile (optimized, smaller wasm bundle)
    #[arg(long)]
    release: bool,

    /// The native platform to package for (`ios`/`android` through the Tauri CLI; default desktop)
    #[arg(long, value_enum, default_value_t = NativePlatform::Desktop)]
    platform: NativePlatform,

    /// Package desktop installers with `cargo tauri build` instead of only compiling the host
    #[arg(long)]
    bundle: bool,

    /// Build only one of the app's surfaces (a server image wants only `web`)
    #[arg(long, value_enum)]
    surface: Option<BuildSurface>,

    #[command(subcommand)]
    target: Option<BuildTarget>,
}

impl BuildArgs {
    /// `fun build <app> --release --surface web`: what a production package is built from.
    pub(crate) const fn release(app: String) -> Self {
        Self {
            app: Some(app),
            release: true,
            platform: NativePlatform::Desktop,
            bundle: false,
            surface: Some(BuildSurface::Web),
            target: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum BuildSurface {
    /// The browser bundle only: the wasm hydrate build, its stylesheet and assets
    Web,
    /// The native webview bundle and host only
    Native,
}

#[derive(Debug, Subcommand)]
enum BuildTarget {
    /// Build a `#[wasm_bindgen]` crate into a scoped npm package with `wasm-pack`
    Wasm(WasmArgs),
}

/// Execute `fun build`.
///
/// # Errors
/// A [`CliError`] for no or an ambiguous app, a failed web bundle, or a failed `cargo build`.
pub(crate) fn run(args: BuildArgs, ctx: &CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let ui = ctx.ui();

    if let Some(BuildTarget::Wasm(wasm_args)) = args.target {
        return wasm::run(wasm_args, &project, ui);
    }

    let manifest = Manifest::load(&project.manifest_path())?;
    let app = manifest.resolve_app(args.app)?;

    let builds_web =
        manifest.has_surface(&app, Surface::Web) && args.surface != Some(BuildSurface::Native);

    if builds_web {
        let css = manifest.web_css_command(&app);
        build_bundle(&project, &app, args.release, css.as_deref(), ui)?;
    }

    // Against what the app declares, so `--surface web --bundle` is still refused.
    let declares_native = manifest.has_surface(&app, Surface::Native);
    if !declares_native && (args.bundle || args.platform != NativePlatform::Desktop) {
        return Err(CliError::SurfaceNotDeclared {
            app: app.clone(),
            surface: "native".to_string(),
        });
    }

    if declares_native && args.surface != Some(BuildSurface::Web) {
        let css = manifest.web_css_command(&app);
        native::build_native_bundle(&project, &app, args.release, css.as_deref(), ui)?;
        match (args.platform, args.bundle) {
            (NativePlatform::Desktop, false) => {
                native::cargo_build_native(&project, &app, args.release, ui)?;
            }
            (platform, _) => {
                native::cargo_tauri_build(&project, &app, platform, args.release, ui)?;
            }
        }
        if args.bundle || args.platform != NativePlatform::Desktop {
            return Ok(());
        }
    }

    if !has_server_binary(&manifest, &app) {
        return Ok(());
    }
    cargo_build(&project, &app, args.release, ui)
}

/// A native-only app crate is a library hosted by its `native/` sibling.
fn has_server_binary(manifest: &Manifest, app: &str) -> bool {
    [
        Surface::Worker,
        Surface::Socket,
        Surface::Api,
        Surface::Web,
        Surface::Sync,
    ]
    .into_iter()
    .any(|surface| manifest.has_surface(app, surface))
}

/// Compile the app's crate with `cargo build -p <crate>`.
///
/// # Errors
/// [`CliError::BuildFailed`] if it exits non-zero, [`CliError::Io`] if it cannot be launched.
fn cargo_build(project: &Project, app: &str, release: bool, ui: &Ui) -> CliResult<()> {
    let crate_name = app.replace('-', "_");
    ui.section(format!("Building {app}"));
    ui.detail(format!(
        "profile: {}",
        if release { "release" } else { "debug" }
    ));

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
