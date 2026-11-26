//! `fun package`: build an app for release and pack it into one verifiable tar.zst.

mod tree_entries;

use std::path::PathBuf;

use clap::Args;
use fundamental_package::{
    FileMode, PackageEntry, PackageManifest, PackageProfile, PackedPath, write_package,
};

use crate::commands::build::BuildArgs;
use crate::helpers::{Manifest, Project, bin_names, cargo_target_dir, git};
use crate::{CliError, CliResult, CommandContext};
use tree_entries::tree_entries;

#[derive(Debug, Args)]
pub struct PackageArgs {
    /// The app to package (defaults to the project's sole app)
    #[arg(value_name = "APP")]
    app: Option<String>,

    /// Where to write it (default `target/package/<app>-production-<commit>.tar.zst`)
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,
}

/// Build the app, pack it, and say where the package is and its digest.
///
/// # Errors
/// [`CliError::PackageFromChangedTree`] (`CLI_072`), what `fun build --release` refuses, or a
/// package refused by its bounds, paths or files (`PKG_0xx`).
pub(crate) fn run(args: PackageArgs, ctx: &CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let ui = ctx.ui();
    let manifest = Manifest::load(&project.manifest_path())?;
    let app = manifest.resolve_app(args.app)?;
    let profile = PackageProfile::Production;

    refuse_a_changed_tree(&project)?;
    crate::commands::build::run(BuildArgs::release(app.clone()), ctx)?;
    refuse_a_changed_tree(&project)?;
    let commit = git(project.root(), &["rev-parse", "HEAD"])?;

    ui.section(format!("Packing {app} ({})", profile.as_str()));
    let entries = entries(&project, &app)?;
    let packed = PackageManifest::measure(&app, profile, &commit, &entries)?;
    let out = match args.out {
        Some(out) => out,
        None => default_out(&project, &app, profile, &commit)?,
    };
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Io {
            path: parent.display().to_string(),
            reason: e.to_string(),
        })?;
    }
    let written = write_package(&packed, &entries, &out, profile.bounds())?;

    ui.detail(format!("package: {}", written.path.display()));
    ui.detail(format!("blake3:  {}", written.digest));
    ui.detail(format!(
        "{} files, {} bytes packed, {} unpacked, commit {commit}",
        written.manifest.files().len(),
        written.packed,
        written.unpacked
    ));
    Ok(())
}

/// Refuses a checkout that differs from its commit, which the package names.
///
/// # Errors
/// [`CliError::PackageFromChangedTree`] naming the changes.
fn refuse_a_changed_tree(project: &Project) -> CliResult<()> {
    let changes = git(project.root(), &["status", "--porcelain"])?;
    if changes.is_empty() {
        return Ok(());
    }
    Err(CliError::PackageFromChangedTree {
        changes: changes.lines().take(10).collect::<Vec<_>>().join("; "),
    })
}

/// The app's binaries, browser bundle and migrations; never the project's settings.
///
/// # Errors
/// [`CliError::Io`] for an unreadable package directory, or a name a package cannot hold.
fn entries(project: &Project, app: &str) -> CliResult<Vec<PackageEntry>> {
    let release = cargo_target_dir(project.root())?.join("release");
    let mut entries = Vec::new();
    for bin in app_bins(project, app)? {
        entries.push(PackageEntry {
            source: release.join(&bin),
            path: PackedPath::parse(&format!("bin/{bin}"))?,
            mode: FileMode::Executable,
        });
    }
    let app_dir = project.app_dir(app);
    entries.extend(tree_entries(&app_dir.join("dist"), "share/dist")?);
    entries.extend(tree_entries(
        &app_dir.join("migrations"),
        "share/migrations",
    )?);
    Ok(entries)
}

/// The app crate's `[[bin]]`s, or the one named for the crate.
///
/// # Errors
/// [`CliError::Io`] when the app's `Cargo.toml` cannot be read.
fn app_bins(project: &Project, app: &str) -> CliResult<Vec<String>> {
    let path = project.app_dir(app).join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| CliError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let bins = bin_names(&text);
    Ok(if bins.is_empty() {
        vec![app.replace('-', "_")]
    } else {
        bins
    })
}

/// `target/package/<app>-<profile>-<commit, 12 digits>.tar.zst`.
///
/// # Errors
/// As [`cargo_target_dir`].
fn default_out(
    project: &Project,
    app: &str,
    profile: PackageProfile,
    commit: &str,
) -> CliResult<PathBuf> {
    let short = &commit[..commit.len().min(12)];
    Ok(cargo_target_dir(project.root())?
        .join("package")
        .join(format!("{app}-{}-{short}.tar.zst", profile.as_str())))
}
