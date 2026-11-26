//! The browser-bundle builder shared by `fun build` and `fun dev`.

use std::path::{Path, PathBuf};
use std::process::Command;

use fundamental::build::{
    BundleFingerprint, DistSnapshot, IslandsCrate, StepPrint, WasmBuild, bindgen_cli_version,
};

use crate::helpers::{Project, cargo_target_dir};
use crate::{CliError, CliResult};

/// Build the web `app`'s browser bundle into its `dist/`: `css_command`, assets, then the wasm.
///
/// # Errors
/// A [`CliError`] if the `css_command` or the wasm build fails.
pub(crate) fn build_bundle(
    project: &Project,
    app: &str,
    release: bool,
    css_command: Option<&str>,
    ui: &crate::ui::Ui,
) -> CliResult<()> {
    let out_dir = project.app_dir(app).join("dist");
    build_bundle_into(
        project,
        app,
        release,
        css_command,
        ui,
        "hydrate",
        &out_dir,
        &[],
    )
}

/// Build the `app`'s wasm bundle with `feature` (`hydrate` or `tauri`) into `out_dir`.
///
/// # Errors
/// As [`build_bundle`].
#[allow(clippy::too_many_arguments)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
pub(crate) fn build_bundle_into(
    project: &Project,
    app: &str,
    release: bool,
    css_command: Option<&str>,
    ui: &crate::ui::Ui,
    feature: &str,
    out_dir: &Path,
    extra_features: &[&str],
) -> CliResult<()> {
    build_styles_and_assets(project, app, css_command, ui, out_dir)?;
    build_islands(project, app, release, ui, feature, out_dir, extra_features)
}

/// The fast half: `css_command` and public assets, each skipped when its fingerprint is current.
///
/// # Errors
/// [`CliError::CssCommandFailed`], or an I/O error copying assets or writing the fingerprint.
pub(crate) fn build_styles_and_assets(
    project: &Project,
    app: &str,
    css_command: Option<&str>,
    ui: &crate::ui::Ui,
    out_dir: &Path,
) -> CliResult<()> {
    let app_dir = project.app_dir(app);

    // Each step is fingerprinted apart; the record is deleted first so an interrupted build claims nothing.
    let recorded = BundleFingerprint::load(out_dir);
    let mut fingerprint = BundleFingerprint::load(out_dir);
    BundleFingerprint::invalidate(out_dir)?;

    if let Some(command) = css_command {
        let css_roots = css_inputs(project.root(), &app_dir);
        let roots: Vec<&Path> = css_roots.iter().map(PathBuf::as_path).collect();
        let print = StepPrint::of_paths(&roots, command);
        // `css_command` is opaque, so its outputs are learned by watching, to notice a deleted stylesheet.
        let outputs = if recorded.is_current("css", &print, out_dir) {
            ui.detail("styles up to date");
            recorded.outputs_of("css")
        } else {
            let before = DistSnapshot::capture(out_dir);
            run_css_command(command, &app_dir, ui)?;
            before.written_since(out_dir)
        };
        fingerprint.set("css", print, outputs);
    }

    // The fingerprint spans both asset roots, or a shared change leaves a stale dist.
    let shared_assets = project.root().join("assets");
    let assets_dir = app_dir.join("assets");
    let assets_print =
        StepPrint::of_paths(&[shared_assets.as_path(), assets_dir.as_path()], "assets");
    let assets_outputs = if recorded.is_current("assets", &assets_print, out_dir) {
        ui.detail("public assets up to date");
        recorded.outputs_of("assets")
    } else {
        let before = DistSnapshot::capture(out_dir);
        super::assets::copy_assets(project.root(), &app_dir, out_dir, ui)?;
        before.written_since(out_dir)
    };
    fingerprint.set("assets", assets_print, assets_outputs);
    fingerprint.save()?;
    Ok(())
}

/// The slow half: the islands compiled to wasm and bound into `out_dir`, skipped when dep-info says so.
///
/// # Errors
/// A [`CliError`] if the wasm build fails or the fingerprint cannot be written.
#[expect(
    clippy::too_many_arguments,
    reason = "half of build_bundle_into, with the same arguments"
)]
pub(crate) fn build_islands(
    project: &Project,
    app: &str,
    release: bool,
    ui: &crate::ui::Ui,
    feature: &str,
    out_dir: &Path,
    extra_features: &[&str],
) -> CliResult<()> {
    let source = BundleSource::of(&project.app_dir(app), app, feature);
    let profile = if release { "release" } else { "debug" };

    let recorded = BundleFingerprint::load(out_dir);
    let mut fingerprint = BundleFingerprint::load(out_dir);
    BundleFingerprint::invalidate(out_dir)?;

    let mut build = WasmBuild::new(
        &source.manifest_path,
        wasm_target_dir(&cargo_target_dir(project.root())?),
        out_dir,
        &source.crate_name,
    )
    .out_name(&source.out_name)
    .feature(source.feature)
    .release(release);
    for extra in extra_features {
        build = build.feature(*extra);
    }

    // Cargo's own dep-info covers every path the crate reaches, `include_str!` targets included.
    let bindgen = bindgen_cli_version().unwrap_or_else(|_| "unknown".to_string());
    let salt = format!("{feature} {} {profile} {bindgen}", extra_features.join(" "));
    let wasm_print = StepPrint::of_dep_info(&build.dep_info_path(), salt.clone());

    if recorded.is_current("wasm", &wasm_print, out_dir) {
        ui.section(format!("The {feature} bundle for {app} is up to date"));
        fingerprint.set("wasm", wasm_print, recorded.outputs_of("wasm"));
        fingerprint.save()?;
        return Ok(());
    }

    ui.section(format!("Building the {feature} bundle for {app}"));
    ui.detail(format!("profile: {profile}"));
    let before_wasm = DistSnapshot::capture(out_dir);
    let artifacts = build.build()?;

    // Re-read: what matters is the dep-info of what was just compiled.
    fingerprint.set(
        "wasm",
        StepPrint::of_dep_info(&build.dep_info_path(), salt),
        before_wasm.written_since(out_dir),
    );
    fingerprint.save()?;

    ui.status(format!("bundle written to {}", out_dir.display()));
    ui.detail(format!("js:   {}", artifacts.js.display()));
    ui.detail(format!("wasm: {}", artifacts.wasm.display()));
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct BundleSource<'a> {
    manifest_path: PathBuf,
    crate_name: String,
    /// `<out_name>.js`: always the app's, so the document's script tag never changes.
    out_name: String,
    feature: &'a str,
}

impl<'a> BundleSource<'a> {
    /// The app's islands crate when it has one, else the app crate, with `feature`.
    fn of(app_dir: &Path, app: &str, feature: &'a str) -> Self {
        let app_crate = app.replace('-', "_");
        match IslandsCrate::of_app(app_dir, app).filter(|_| feature == "hydrate") {
            Some(islands) => Self {
                manifest_path: islands.manifest_path().to_path_buf(),
                crate_name: islands.crate_name().to_owned(),
                out_name: app_crate,
                feature,
            },
            None => Self {
                manifest_path: app_dir.join("Cargo.toml"),
                crate_name: app_crate.clone(),
                out_name: app_crate,
                feature,
            },
        }
    }
}

/// Its own directory inside cargo's, so the wasm and server builds hold different locks.
fn wasm_target_dir(cargo_target: &Path) -> PathBuf {
    cargo_target.join("wasm")
}

/// A heuristic over the app, the shared `styles/` and `domain/` crates; `--force` covers the rest.
fn css_inputs(project_root: &Path, app_dir: &Path) -> [PathBuf; 5] {
    [
        app_dir.join("styles"),
        app_dir.join("src"),
        app_dir.join("pages"),
        project_root.join("styles"),
        project_root.join("domain"),
    ]
}

/// Run the app's `css_command` shell line in `app_dir`.
fn run_css_command(command: &str, app_dir: &std::path::Path, ui: &crate::ui::Ui) -> CliResult<()> {
    ui.section("Building styles");
    ui.detail(command.to_string());
    let mut shell = Command::new("sh");
    if command.contains("tailwindcss") {
        let pinned = fundamental::build::tailwind::pinned_dir().map_err(|e| CliError::Io {
            path: "tailwindcss".to_string(),
            reason: e.to_string(),
        })?;
        ui.detail(format!(
            "tailwindcss {} from {}",
            fundamental::build::tailwind::VERSION,
            pinned.display()
        ));
        let path = std::env::var_os("PATH").unwrap_or_default();
        let joined =
            std::env::join_paths(std::iter::once(pinned).chain(std::env::split_paths(&path)))
                .map_err(|e| CliError::Io {
                    path: "PATH".to_string(),
                    reason: format!("could not put the pinned tailwindcss first on PATH: {e}"),
                })?;
        shell.env("PATH", joined);
    }
    let status = shell
        .arg("-c")
        .arg(command)
        .current_dir(app_dir)
        .status()
        .map_err(|e| CliError::Io {
            path: "sh".to_string(),
            reason: format!("could not run css_command: {e}"),
        })?;
    if !status.success() {
        return Err(CliError::CssCommandFailed {
            code: status.code().unwrap_or(-1),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn an_app_without_an_islands_crate_builds_its_bundle_from_the_app_crate() {
        let app = tempfile::tempdir().expect("an app");
        let source = BundleSource::of(app.path(), "my-site", "hydrate");
        assert_eq!(source.manifest_path, app.path().join("Cargo.toml"));
        assert_eq!(source.crate_name, "my_site");
        assert_eq!(source.out_name, "my_site");
        assert_eq!(source.feature, "hydrate");
    }

    #[test]
    fn an_islands_crate_builds_the_bundle_under_the_apps_name() {
        let app = tempfile::tempdir().expect("an app");
        fs::create_dir_all(app.path().join("islands")).expect("the islands");
        fs::write(app.path().join("islands/Cargo.toml"), "[package]").expect("manifest");
        let source = BundleSource::of(app.path(), "my-site", "hydrate");
        assert_eq!(source.manifest_path, app.path().join("islands/Cargo.toml"));
        assert_eq!(source.crate_name, "my_site_islands");
        assert_eq!(
            source.out_name, "my_site",
            "/_fundamental/my_site.js keeps its name"
        );
        assert_eq!(source.feature, "hydrate");
    }

    #[test]
    fn a_native_webview_bundle_is_built_from_the_app_crate_still() {
        let app = tempfile::tempdir().expect("an app");
        fs::create_dir_all(app.path().join("islands")).expect("the islands");
        fs::write(app.path().join("islands/Cargo.toml"), "[package]").expect("manifest");
        let source = BundleSource::of(app.path(), "my-site", "tauri");
        assert_eq!(source.manifest_path, app.path().join("Cargo.toml"));
        assert_eq!(source.feature, "tauri");
    }

    #[test]
    fn the_wasm_build_compiles_in_a_directory_of_its_own() {
        assert_eq!(
            wasm_target_dir(Path::new("/p/target")),
            Path::new("/p/target/wasm")
        );
    }

    #[test]
    fn an_edit_to_a_domain_crates_stylesheet_reruns_the_css_step() {
        let root = tempfile::tempdir().expect("a project");
        let app = root.path().join("apps/www");
        fs::create_dir_all(app.join("styles")).expect("the app's styles");
        fs::write(
            app.join("styles/input.css"),
            "@import \"../../../domain/brand/styles/brand.css\";",
        )
        .expect("input");
        let brand = root.path().join("domain/brand/styles");
        fs::create_dir_all(&brand).expect("the brand's styles");
        fs::write(brand.join("brand.css"), ".block{display:grid}").expect("brand");
        let print = || {
            let roots = css_inputs(root.path(), &app);
            let roots: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
            StepPrint::of_paths(&roots, "tailwindcss")
        };
        let before = print();
        fs::write(brand.join("brand.css"), ".block{display:grid;gap:4px}").expect("an edit");
        assert_ne!(
            print(),
            before,
            "the brand's stylesheet is one of the css step's inputs"
        );
    }

    #[test]
    fn a_class_written_in_a_domain_crates_component_reruns_the_css_step() {
        let root = tempfile::tempdir().expect("a project");
        let app = root.path().join("apps/www");
        let component = root.path().join("domain/brand/src");
        fs::create_dir_all(&component).expect("the brand's components");
        fs::write(component.join("badge.rs"), "class=\"badge\"").expect("a component");
        let roots = css_inputs(root.path(), &app);
        let roots: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        let before = StepPrint::of_paths(&roots, "tailwindcss");
        fs::write(component.join("badge.rs"), "class=\"badge mt-2\"").expect("a new class");
        assert_ne!(StepPrint::of_paths(&roots, "tailwindcss"), before);
    }
}
