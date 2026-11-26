//! `fun dev`: run one app or a set from one cargo invocation, rebuilding and restarting on change.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use clap::{Args, ValueEnum};
use fundamental::config::loader::INSTANCE_ENV;
use fundamental_dev::{BuildPlan, DevApp, DevLoop};
use tracing::info;

use crate::commands::build::native::{
    NativePlatform, build_native_bundle, ensure_mobile_project, ensure_tauri_cli,
    native_crate_name, native_dir,
};
use crate::commands::build::web::{build_islands, build_styles_and_assets};
use crate::helpers::{Manifest, Project, cargo_target_dir, resolve_main_bin};

mod dev_instance;

use crate::{CliError, CliResult, CommandContext, Surface};
use dev_instance::DevInstance;

const INTROSPECT: &str = "introspect";

/// Arguments for `fun dev`.
#[derive(Debug, Args)]
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
pub struct DevArgs {
    /// The apps to run, or one app and some of its surfaces (`fun dev main web`; `native` runs alone)
    #[arg(value_name = "APP|SURFACE")]
    targets: Vec<String>,

    /// Build and run with the release profile.
    #[arg(long)]
    release: bool,

    /// Watch the sources and rebuild and restart on change (default; `--watch false` builds once)
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    watch: bool,

    /// Build once, run, and supervise without rebuilding; the later of this and `--watch` wins
    #[arg(long, overrides_with = "watch")]
    no_watch: bool,

    /// Extra directories to watch beyond `domain/`, `crates/` and `apps/` (repeatable)
    #[arg(long = "watch-path", value_name = "PATH")]
    watch_paths: Vec<PathBuf>,

    /// Milliseconds the tree must be quiet before a change triggers a rebuild.
    #[arg(long, default_value_t = 250)]
    debounce: u64,

    /// Seconds a restarting app may take to drain before it is killed.
    #[arg(long, default_value_t = 10)]
    grace: u64,

    /// Print the exact cargo selection this would build, to share with scripts and CI, and exit
    #[arg(long)]
    print_cargo_args: bool,

    /// Where to run the native surface (with `native`; default: desktop).
    #[arg(long, value_enum, default_value_t = NativePlatform::Desktop)]
    platform: NativePlatform,

    /// The device or simulator for a mobile run (default: the Tauri CLI's own picker)
    #[arg(long)]
    device: Option<String>,

    /// Serve as the local lane with this label (`fun local`); CI names its clone `ci`
    #[arg(long, env = "FUN_LOCAL_LABEL")]
    local_label: Option<String>,
}

impl DevArgs {
    fn watching(&self) -> bool {
        self.watch && !self.no_watch
    }
}

/// Execute `fun dev`.
///
/// # Errors
/// A [`CliError`] for an unknown target, no or an ambiguous app, a surface subset with several
/// apps or an undeclared surface, a failed bundle build, or a dev loop that cannot start.
pub(crate) fn run(args: DevArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let manifest = Manifest::load(&project.manifest_path())?;
    let (apps, surfaces) = split_targets(&manifest, &args.targets)?;

    // A surface subset is a property of one app.
    if apps.len() > 1 {
        if !surfaces.is_empty() {
            return Err(CliError::SurfaceSubsetNeedsOneApp {
                apps: apps.join(", "),
                surfaces: surfaces
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            });
        }
        return run_set(&args, &project, &manifest, apps, ctx);
    }

    let app = manifest.resolve_app(apps.into_iter().next())?;

    for surface in &surfaces {
        if !manifest.has_surface(&app, *surface) {
            return Err(CliError::SurfaceNotDeclared {
                app: app.clone(),
                surface: surface.as_str().to_string(),
            });
        }
    }
    // The native surface is its own process with its own tooling's dev loop, so it runs alone.
    if surfaces.contains(&Surface::Native) {
        if surfaces.len() > 1 {
            return Err(CliError::SurfaceRunsAlone {
                app,
                surface: "native".to_owned(),
            });
        }
        return run_native(&args, &project, &manifest, &app, ctx.ui());
    }

    run_server_set(&args, &project, &manifest, vec![app], &surfaces, ctx)
}

fn run_set(
    args: &DevArgs,
    project: &Project,
    manifest: &Manifest,
    apps: Vec<String>,
    ctx: CommandContext,
) -> CliResult<()> {
    run_server_set(args, project, manifest, apps, &[], ctx)
}

/// One cargo invocation, so the set shares one feature resolution and its artifacts.
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
fn run_server_set(
    args: &DevArgs,
    project: &Project,
    manifest: &Manifest,
    apps: Vec<String>,
    surfaces: &[Surface],
    ctx: CommandContext,
) -> CliResult<()> {
    let ui = ctx.ui();

    let web_apps = web_apps(manifest, &apps, surfaces);

    if args.print_cargo_args {
        let plan = build_plan(args, project, manifest, &apps, surfaces, None)?;
        // The only output, so the line can be captured verbatim.
        println!("cargo {}", plan.cargo_args().join(" "));
        return Ok(());
    }

    let instance = DevInstance::up(project.root(), args.local_label.as_deref(), ui)?;
    let plan = build_plan(args, project, manifest, &apps, surfaces, instance.as_ref())?;

    ui.section(match surface_list(surfaces) {
        Some(list) => format!("Running {} [{list}] (Ctrl-C to stop)", apps.join(", ")),
        None => format!("Running {} (Ctrl-C to stop)", apps.join(", ")),
    });
    ui.detail(format!("cargo {}", plan.cargo_args().join(" ")));

    let mut dev = DevLoop::new(project.root(), cargo_target_dir(project.root())?, plan)
        .watch(args.watching())
        .debounce(Duration::from_millis(args.debounce))
        .grace(Duration::from_secs(args.grace));

    if args.watching() {
        let roots = watch_roots(project, &args.watch_paths);
        ui.detail(format!(
            "watching {}",
            roots
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        dev = dev.watch_roots(roots);
    }

    if !web_apps.is_empty() {
        let bases = bundle_bases(manifest, &web_apps);
        dev = with_bundle_steps(dev, project, bases, args.release, *ui);
    }

    dev.run().map_err(CliError::from)
}

/// The bundle must exist before its server starts and be refreshed by a rebuild, so it runs in the loop.
fn with_bundle_steps(
    mut dev: DevLoop,
    project: &Project,
    css: Vec<(String, Option<String>)>,
    release: bool,
    ui: crate::ui::Ui,
) -> DevLoop {
    let project = project.clone();
    let root = project.root().to_path_buf();
    let bases: Vec<String> = css.iter().map(|(app, _)| app.clone()).collect();
    let styled = project.clone();
    // Styles and assets first and alone: they take under a second.
    dev = dev.prebuild(move || {
        for (app, css_command) in &css {
            let dist = styled.app_dir(app).join("dist");
            build_styles_and_assets(&styled, app, css_command.as_deref(), &ui, &dist)
                .map_err(dev_error)?;
        }
        Ok(())
    });
    // Islands beside the server: each compiles in its own target directory.
    dev.alongside(move || {
        for app in &bases {
            let dist = project.app_dir(app).join("dist");
            build_islands(&project, app, release, &ui, "hydrate", &dist, &[]).map_err(dev_error)?;
        }
        Ok(())
    })
    .style_only(move |path| is_stylesheet(path.strip_prefix(&root).unwrap_or(path)))
}

/// A `.css` file in a `styles` tree; one elsewhere may be `include_str!`-ed into a binary.
fn is_stylesheet(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|ext| ext == "css")
        && path
            .parent()
            .is_some_and(|dir| dir.components().any(|part| part.as_os_str() == "styles"))
}

/// Each app whose bundle is built, with its `css_command`; an instance serves its base's.
fn bundle_bases(manifest: &Manifest, web_apps: &[String]) -> Vec<(String, Option<String>)> {
    let mut css: Vec<(String, Option<String>)> = Vec::new();
    for app in web_apps {
        let base = manifest.instance_base(app).unwrap_or_else(|| app.clone());
        if !css.iter().any(|(built, _)| *built == base) {
            css.push((base, manifest.web_css_command(app)));
        }
    }
    css
}

fn dev_error(error: CliError) -> fundamental_dev::DevError {
    fundamental_dev::DevError::Other(fundamental::core::AnyError::new(error))
}

fn web_apps(manifest: &Manifest, apps: &[String], surfaces: &[Surface]) -> Vec<String> {
    apps.iter()
        .filter(|app| {
            manifest.has_surface(app, Surface::Web)
                && (surfaces.is_empty() || surfaces.contains(&Surface::Web))
        })
        .cloned()
        .collect()
}

/// One `DevApp` per app, with `[dev] features` when declared, else `introspect` for HTTP apps.
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
fn build_plan(
    args: &DevArgs,
    project: &Project,
    manifest: &Manifest,
    apps: &[String],
    surfaces: &[Surface],
    instance: Option<&DevInstance>,
) -> CliResult<BuildPlan> {
    let declared = manifest.dev_features();

    let dev_apps = apps
        .iter()
        .map(|app| {
            // An instance runs its base's binary, keeping its own name for its variables and origins.
            let base = manifest.instance_base(app);
            let code = base.as_deref().unwrap_or(app);
            let crate_name = code.replace('-', "_");
            // `layer5_witness` produces `witness`, so the binary is resolved from the package.
            let mut dev_app = DevApp::new(app.clone())
                .package(crate_name)
                .bin(resolve_main_bin(project, code)?);
            // Never from a `FUNDAMENTAL_INSTANCE` the shell happens to export.
            if let Some(base) = &base {
                info!(instance = %app, base = %base, "fun dev: running an instance as its base's binary");
                dev_app = dev_app.env(INSTANCE_ENV, app.clone());
            } else {
                dev_app = dev_app.env(INSTANCE_ENV, "");
            }

            let features = declared.clone().unwrap_or_else(|| {
                // `fun build` never enables it, so it is absent from release binaries.
                if manifest.has_surface(app, Surface::Web)
                    || manifest.has_surface(app, Surface::Api)
                {
                    vec![INTROSPECT.to_string()]
                } else {
                    Vec::new()
                }
            });
            for feature in features {
                dev_app = dev_app.feature(feature);
            }

            if let Some(list) = surface_list(surfaces) {
                dev_app = dev_app.env("FUNDAMENTAL_SURFACES", list);
            }
            // For apps scaffolded before the template defaulted to `info`.
            if std::env::var_os("RUST_LOG").is_none() {
                dev_app = dev_app.env("RUST_LOG", "info");
            }
            // Applied to this app's child only: the process-wide loader would let the first app win for all.
            dev_app = dev_app.envs(fundamental::config::dev_vars_for_app(project.root(), app));
            // Last: the instance's facts of this checkout outrank a value written for another.
            if let Some(instance) = instance {
                dev_app = instance.apply(dev_app);
            }
            Ok(dev_app)
        })
        .collect::<CliResult<Vec<DevApp>>>()?;

    Ok(BuildPlan::new(dev_apps).release(args.release))
}

/// The project's source trees plus `--watch-path`; never the root, which holds volumes and output.
fn watch_roots(project: &Project, extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for dir in ["domain", "crates", "apps", "assets", "styles"] {
        let path = project.root().join(dir);
        if path.exists() {
            roots.push(path);
        }
    }
    for file in ["Cargo.toml", "Cargo.lock", "fun.toml", "fun.local.toml"] {
        let path = project.root().join(file);
        if path.exists() {
            roots.push(path);
        }
    }
    roots.extend(extra.iter().cloned());
    roots
}

/// Split targets into apps and surfaces; a declared app wins over a same-named surface.
fn split_targets(
    manifest: &Manifest,
    targets: &[String],
) -> CliResult<(Vec<String>, Vec<Surface>)> {
    let mut apps = Vec::new();
    let mut surfaces = Vec::new();
    for target in targets {
        if manifest.has_app(target) {
            apps.push(target.clone());
            continue;
        }
        match Surface::from_str(target, true) {
            Ok(surface) => surfaces.push(surface),
            // A typo here is almost always of an app.
            Err(_) => {
                return Err(CliError::AppNotFound {
                    app: target.clone(),
                });
            }
        }
    }
    Ok((apps, surfaces))
}

/// Build the webview bundle, then `cargo run` on desktop or `cargo tauri ios|android dev` on mobile.
fn run_native(
    args: &DevArgs,
    project: &Project,
    manifest: &Manifest,
    app: &str,
    ui: &crate::ui::Ui,
) -> CliResult<()> {
    let css = manifest.web_css_command(app);
    build_native_bundle(project, app, args.release, css.as_deref(), ui)?;

    match args.platform {
        NativePlatform::Desktop => {
            ui.section(format!("Running {app} native (desktop; Ctrl-C to stop)"));
            let mut cargo = cargo_run(project, &native_crate_name(app), args.release);
            let status = cargo.status().map_err(|e| CliError::Io {
                path: "cargo".to_string(),
                reason: format!("could not run the native app: {e}"),
            })?;
            if !status.success() {
                return Err(CliError::DevServerFailed {
                    code: status.code().unwrap_or(-1),
                });
            }
            Ok(())
        }
        NativePlatform::Ios | NativePlatform::Android => {
            let platform = if args.platform == NativePlatform::Ios {
                "ios"
            } else {
                "android"
            };
            ensure_tauri_cli()?;
            let dir = native_dir(project, app);
            ensure_mobile_project(&dir, platform, ui)?;

            ui.section(format!("Running {app} native ({platform}; Ctrl-C to stop)"));
            let mut cargo = Command::new("cargo");
            cargo.args(["tauri", platform, "dev"]).current_dir(&dir);
            if args.release {
                cargo.arg("--release");
            }
            if let Some(device) = &args.device {
                cargo.arg(device);
            }
            let status = cargo.status().map_err(|e| CliError::Io {
                path: "cargo tauri".to_string(),
                reason: format!("could not run cargo tauri {platform} dev: {e}"),
            })?;
            if !status.success() {
                return Err(CliError::DevServerFailed {
                    code: status.code().unwrap_or(-1),
                });
            }
            Ok(())
        }
    }
}

/// `cargo run -p <name> --bin <name>`, so an auxiliary binary does not make it ambiguous (CLI_022).
fn cargo_run(project: &Project, crate_name: &str, release: bool) -> Command {
    let mut cargo = Command::new("cargo");
    cargo
        .arg("run")
        .arg("-p")
        .arg(crate_name)
        .arg("--bin")
        .arg(crate_name)
        .current_dir(project.root());
    if release {
        cargo.arg("--release");
    }
    default_dev_log(&mut cargo);
    cargo
}

/// A default `RUST_LOG` of `info` when the environment sets none.
fn default_dev_log(cargo: &mut Command) {
    if std::env::var_os("RUST_LOG").is_none() {
        cargo.env("RUST_LOG", "info");
    }
}

/// `None` when none were named, meaning every declared surface runs.
fn surface_list(surfaces: &[Surface]) -> Option<String> {
    if surfaces.is_empty() {
        return None;
    }
    Some(
        surfaces
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(","),
    )
}

#[cfg(test)]
mod tests {
    use super::{DevArgs, cargo_run};
    use crate::helpers::{Manifest, Project};
    use clap::Parser;
    use tempfile::TempDir;

    #[derive(Parser)]
    struct Probe {
        #[command(flatten)]
        dev: DevArgs,
    }

    fn watching(argv: &[&str]) -> bool {
        Probe::try_parse_from(std::iter::once("dev").chain(argv.iter().copied()))
            .expect("the command line parses")
            .dev
            .watching()
    }

    #[test]
    fn watching_is_the_default_and_both_spellings_turn_it_off() {
        assert!(watching(&[]));
        assert!(watching(&["--watch", "true"]));
        assert!(!watching(&["--watch", "false"]));
        assert!(!watching(&["--no-watch"]));
    }

    #[test]
    fn the_later_of_watch_and_no_watch_wins() {
        assert!(watching(&["--no-watch", "--watch", "true"]));
        assert!(!watching(&["--watch", "true", "--no-watch"]));
    }

    fn manifest_with(body: &str) -> (TempDir, Manifest) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("fun.toml");
        std::fs::write(&path, body).unwrap();
        let manifest = Manifest::load(&path).unwrap();
        (tmp, manifest)
    }

    fn project() -> (TempDir, Project) {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("fun.toml"), "[project]\nname = \"acme\"\n").unwrap();
        let project = Project::discover_from(tmp.path()).unwrap();
        (tmp, project)
    }

    fn args(cargo: &std::process::Command) -> Vec<String> {
        cargo
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn cargo_run_selects_the_named_binary() {
        let (_t, project) = project();
        let cargo = cargo_run(&project, "layer0", false);
        let args = args(&cargo);

        let bin = args
            .iter()
            .position(|a| a == "--bin")
            .expect("--bin passed");
        assert_eq!(args.get(bin + 1).map(String::as_str), Some("layer0"));
        let pkg = args.iter().position(|a| a == "-p").expect("-p passed");
        assert_eq!(args.get(pkg + 1).map(String::as_str), Some("layer0"));
        assert_eq!(args.first().map(String::as_str), Some("run"));
    }

    #[test]
    fn cargo_run_appends_release_only_when_asked() {
        let (_t, project) = project();
        assert!(!args(&cargo_run(&project, "layer0", false)).contains(&"--release".to_string()));
        assert!(args(&cargo_run(&project, "layer0", true)).contains(&"--release".to_string()));
    }

    #[test]
    fn fun_dev_runs_an_instance_as_its_bases_binary() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("fun.toml"),
            "[project]\nname = \"acme\"\n\n[apps.www]\nweb = { port = 3000, css_command = \"true\" }\n\n[apps.www_days]\ninstance_of = \"www\"\nweb = { port = 3001 }\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join("apps/www")).unwrap();
        std::fs::write(
            tmp.path().join("apps/www/Cargo.toml"),
            "[package]\nname = \"www\"\n\n[[bin]]\nname = \"www\"\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join(".dev.vars.www"), "WHO=www\n").unwrap();
        std::fs::write(tmp.path().join(".dev.vars.www_days"), "WHO=www_days\n").unwrap();
        let project = Project::discover_from(tmp.path()).unwrap();
        let manifest = Manifest::load(&tmp.path().join("fun.toml")).unwrap();
        assert!(manifest.has_surface("www_days", crate::Surface::Web));
        assert_eq!(
            manifest.web_css_command("www_days").as_deref(),
            Some("true")
        );
        let args = Probe::try_parse_from(["dev"]).unwrap().dev;
        let plan = super::build_plan(
            &args,
            &project,
            &manifest,
            &["www".to_owned(), "www_days".to_owned()],
            &[],
            None,
        )
        .expect("a plan");
        let [www, days] = plan.apps() else {
            panic!("two apps: {:?}", plan.apps());
        };
        assert_eq!((www.package.as_str(), www.bin.as_str()), ("www", "www"));
        assert_eq!(
            (days.name.as_str(), days.package.as_str(), days.bin.as_str()),
            ("www_days", "www", "www")
        );
        assert_eq!(
            days.env.get("FUNDAMENTAL_INSTANCE").map(String::as_str),
            Some("www_days")
        );
        assert_eq!(
            www.env.get("FUNDAMENTAL_INSTANCE").map(String::as_str),
            Some(""),
            "an app runs as itself whatever the shell exports"
        );
        assert_eq!(
            days.env.get("WHO").map(String::as_str),
            Some("www_days"),
            "its own variables"
        );
        assert_eq!(www.env.get("WHO").map(String::as_str), Some("www"));
        let selected = plan.cargo_args();
        assert_eq!(
            selected.iter().filter(|arg| *arg == "-p").count(),
            1,
            "{selected:?}"
        );
    }

    #[test]
    fn targets_split_into_apps_and_surfaces_against_the_manifest() {
        let (_t, manifest) = manifest_with(
            "[project]\nname = \"acme\"\n\n[apps.layer0]\napi = { port = 16000 }\n\n[apps.main]\nweb = { port = 11001 }\n",
        );
        let (apps, surfaces) =
            super::split_targets(&manifest, &["main".into(), "web".into()]).expect("split");
        assert_eq!(apps, vec!["main".to_string()]);
        assert_eq!(surfaces, vec![super::Surface::Web]);

        let (apps, surfaces) =
            super::split_targets(&manifest, &["layer0".into(), "main".into()]).expect("split");
        assert_eq!(apps.len(), 2);
        assert!(surfaces.is_empty());
    }

    #[test]
    fn an_app_named_like_a_surface_is_still_the_app() {
        let (_t, manifest) =
            manifest_with("[project]\nname = \"acme\"\n\n[apps.web]\napi = { port = 16000 }\n");
        let (apps, surfaces) = super::split_targets(&manifest, &["web".into()]).expect("split");
        assert_eq!(apps, vec!["web".to_string()]);
        assert!(surfaces.is_empty());
    }

    #[test]
    fn a_target_that_is_neither_is_a_typed_error() {
        use fundamental::core::Error as _;
        let (_t, manifest) =
            manifest_with("[project]\nname = \"acme\"\n\n[apps.main]\nweb = { port = 11001 }\n");
        let err = super::split_targets(&manifest, &["layer9".into()]).expect_err("unknown");
        assert_eq!(err.error_code(), "CLI_009");
    }

    #[test]
    fn dev_watches_every_settings_file() {
        let (_t, project) = project();
        std::fs::write(project.root().join("fun.local.toml"), "").unwrap();
        let roots = super::watch_roots(&project, &[]);
        for file in ["fun.toml", "fun.local.toml"] {
            assert!(roots.iter().any(|r| r.ends_with(file)), "{file}: {roots:?}");
        }
    }

    #[test]
    fn a_stylesheet_under_a_styles_tree_is_answered_by_the_styles_step_alone() {
        use std::path::Path;
        assert!(super::is_stylesheet(Path::new(
            "domain/brand/styles/brand.css"
        )));
        assert!(super::is_stylesheet(Path::new("apps/www/styles/input.css")));
        assert!(super::is_stylesheet(Path::new("styles/app.css")));
        assert!(!super::is_stylesheet(Path::new("domain/brand/src/lib.rs")));
        assert!(!super::is_stylesheet(Path::new("crates/ui/src/look.css")));
    }

    #[test]
    fn watch_roots_are_the_source_trees_that_exist() {
        let (_t, project) = project();
        std::fs::create_dir_all(project.root().join("domain")).unwrap();
        std::fs::create_dir_all(project.root().join("apps")).unwrap();
        std::fs::create_dir_all(project.root().join("assets")).unwrap();
        std::fs::create_dir_all(project.root().join("styles")).unwrap();
        std::fs::create_dir_all(project.root().join("volumes/sql")).unwrap();

        let roots = super::watch_roots(&project, &[]);
        assert!(roots.iter().any(|r| r.ends_with("domain")));
        assert!(roots.iter().any(|r| r.ends_with("apps")));
        assert!(roots.iter().any(|r| r.ends_with("fun.toml")));
        assert!(roots.iter().any(|r| r.ends_with("assets")));
        assert!(roots.iter().any(|r| r.ends_with("styles")));
        assert!(
            !roots.iter().any(|r| r.ends_with("volumes")),
            "database volumes must never be walked: {roots:?}"
        );
        assert!(!roots.iter().any(|r| r.ends_with("crates")));
    }

    #[test]
    fn extra_watch_paths_are_appended() {
        let (_t, project) = project();
        let extra = std::path::PathBuf::from("/elsewhere/fundamental");
        let roots = super::watch_roots(&project, std::slice::from_ref(&extra));
        assert!(roots.contains(&extra));
    }
}
