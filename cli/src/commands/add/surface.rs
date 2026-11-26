//! `fun add surface <kind>`: add a runtime surface to an existing app.

use std::path::Path;

use super::AddSurfaceArgs;
use super::app_lib;
use crate::commands::scaffold::{PlanResolver, ScaffoldPrompts};
use crate::helpers::{DomainCrate, Manifest, Project};
use crate::ui::Ui;
use crate::{CliError, CliResult, CommandContext, Surface};

const TAILWIND_CSS_COMMAND: &str = "tailwindcss -i styles/input.css -o dist/app.css";

/// Register the surface in `fun.toml` and add its builder line, handlers and feature to the app.
///
/// # Errors
/// A [`CliError`] if the project or app cannot be found, or the surface is already declared.
pub(crate) fn run(args: AddSurfaceArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let mut manifest = Manifest::load(&project.manifest_path())?;

    if !manifest.has_app(&args.app) {
        return Err(CliError::AppNotFound {
            app: args.app.clone(),
        });
    }
    if manifest.has_surface(&args.app, args.kind) {
        return Err(CliError::AlreadyExists {
            what: "surface".to_string(),
            name: format!("{}.{}", args.app, args.kind.as_str()),
        });
    }

    let prompts = ScaffoldPrompts::new(ctx);
    let resolver = PlanResolver::new(&prompts, console::user_attended());
    let tailwind = resolver.tailwind(args.tailwind, &[args.kind])?;

    // Matches the `fun new` defaults (worker ignores it).
    manifest.add_surface(&args.app, args.kind, args.kind.default_port())?;
    if tailwind {
        manifest.set_web_css_command(&args.app, TAILWIND_CSS_COMMAND)?;
    }
    manifest.save()?;

    // The native surface is a sibling crate of all-new files, so it is generated outright.
    if args.kind == Surface::Native {
        generate_native_tree(&project, &manifest, &args.app)?;
    }
    // Only surfaces that depend on the domain crate ask which one, so a worker needs no `--domain`.
    let domain = match args.kind {
        Surface::Web => {
            let dir = project.resolve_domain_crate(args.domain.as_deref())?;
            DomainCrate::read(project.root(), &dir)?
        }
        _ => DomainCrate::scaffolded(),
    };
    if args.kind == Surface::Web {
        domain.require(&["server"])?;
        let app_crate_bundle = manifest.has_surface(&args.app, Surface::Sync)
            || manifest.has_surface(&args.app, Surface::Native);
        if !app_crate_bundle && write_islands_crate(project.root(), &args.app, &domain)? {
            prompts.ui().status(format!(
                "wrote apps/{}/islands: the browser bundle is built from it; keep each `#[island]` in a crate it uses, never in pages/",
                args.app
            ));
        }
    }

    let web = manifest.has_surface(&args.app, Surface::Web);
    let added = add_to_app(&project, &args.app, args.kind, web)?;
    let ui = prompts.ui();
    ui.blank();
    ui.section(format!(
        "Added {} surface to {}",
        args.kind.as_str(),
        args.app
    ));
    ui.blank();
    match args.kind {
        Surface::Native => print_native_guidance(ui, &args.app),
        Surface::Web => print_web_guidance(ui, &args.app, tailwind, &domain),
        kind => print_app_guidance(ui, &args.app, kind, added)?,
    }
    ui.blank();
    Ok(())
}

/// Adds `kind` to the app's `App` and its feature to the manifest, answering whether it could.
///
/// # Errors
/// [`CliError::Io`] if the library or the manifest cannot be read or written.
fn add_to_app(project: &Project, app: &str, kind: Surface, web: bool) -> CliResult<bool> {
    use crate::helpers::write_file;

    if matches!(kind, Surface::Native | Surface::Web) {
        return Ok(false);
    }
    let dir = project.app_dir(app);
    let lib_path = dir.join("src").join("lib.rs");
    let Ok(lib) = std::fs::read_to_string(&lib_path) else {
        return Ok(false);
    };
    let Some(added) = app_lib::add_surface(&lib, app, kind, web) else {
        return Ok(false);
    };
    write_file(&lib_path, &added)?;
    let manifest_path = dir.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path).map_err(|e| CliError::Io {
        path: manifest_path.display().to_string(),
        reason: e.to_string(),
    })?;
    let asked = app_lib::add_feature(&manifest, kind, web);
    let manifest = asked.as_deref().unwrap_or(&manifest);
    if let Some(manifest) = app_lib::add_domain_feature(manifest, kind, web).or(asked) {
        write_file(&manifest_path, &manifest)?;
    }
    Ok(true)
}

/// Write `apps/<app>/islands` and register it; one already there is left alone (`Ok(false)`).
///
/// # Errors
/// [`CliError::Io`] if a file or the workspace manifest cannot be read, parsed or written.
fn write_islands_crate(root: &Path, app: &str, domain: &DomainCrate) -> CliResult<bool> {
    use crate::commands::new::{islands_cargo_toml, islands_lib_rs};
    use crate::helpers::{add_workspace_member, write_file};

    let islands = root.join("apps").join(app).join("islands");
    if islands.join("Cargo.toml").exists() {
        return Ok(false);
    }
    let app_crate = app.replace('-', "_");
    write_file(
        &islands.join("Cargo.toml"),
        &islands_cargo_toml(&app_crate, domain),
    )?;
    write_file(
        &islands.join("src").join("lib.rs"),
        &islands_lib_rs(app, &app_crate),
    )?;
    add_workspace_member(&root.join("Cargo.toml"), &format!("apps/{app}/islands"))?;
    Ok(true)
}

fn generate_native_tree(project: &Project, manifest: &Manifest, app: &str) -> CliResult<()> {
    use crate::commands::new::generate_native_crate;
    use crate::commands::scaffold::ProjectPlan;
    use crate::helpers::add_workspace_member;

    let root = project.root().to_path_buf();
    let name = manifest.project_name().unwrap_or_else(|| {
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "app".to_owned())
    });
    let plan = ProjectPlan::new(&name, root.clone(), &[Surface::Native]).with_app(app);
    generate_native_crate(&plan, &root.join("apps").join(app).join("native"))?;
    let workspace = project.workspace_manifest_path();
    add_workspace_member(&workspace, &format!("apps/{app}/native"))?;
    add_workspace_member(&workspace, &format!("apps/{app}/native/hooks"))?;
    Ok(())
}

/// What is left for an added surface, or the exact line to add when the library has no `app()`.
///
/// # Errors
/// [`CliError::Internal`] for a surface that is not one builder line (web, native).
fn print_app_guidance(ui: &Ui, app: &str, kind: Surface, added: bool) -> CliResult<()> {
    let line =
        crate::commands::new::surface_line(kind, false).ok_or_else(|| CliError::Internal {
            detail: format!("{} surface has no builder line", kind.as_str()),
        })?;
    if added {
        ui.status(format!(
            "added `{}` to app() in apps/{app}/src/lib.rs, with its handlers and its features",
            line.lines().next().unwrap_or_default().trim()
        ));
    } else {
        ui.section(format!(
            "Add it to the `App` in apps/{app}/src/lib.rs, after `fundamental::app!()`"
        ));
        for code in line.lines() {
            ui.code(code.trim());
        }
        if kind == Surface::Sync {
            ui.note("enable `domain/sync-server` on the app's domain dependency");
        }
    }
    print_surface_notes(ui, kind);
    Ok(())
}

fn print_surface_notes(ui: &Ui, kind: Surface) {
    match kind {
        Surface::Worker => {
            ui.note("it connects to NATS from `[messaging.nats]` when it starts; list each `#[subscribe]` handler in `mount_workers!`");
        }
        Surface::Socket => {
            ui.note("the runtime drains it on SIGTERM: every connection is told 1001 and a running handler finishes first");
            ui.note("`.socket_config(WsConfig::public)` serves every connection without authenticating; without it, a connection that binds no principal (`WsContext::bind_principal`) within ten seconds is closed");
        }
        Surface::Api => {
            ui.note("its routes are served over the app's state, at `[apps.<app>.api]`; a test answers them with `app().api_router()`");
        }
        Surface::Sync => {
            ui.note("list each slice in `mount_slices!`");
            ui.note("it serves the app's default SQL pool, or its default CQL keyspace when it declares no SQL database");
        }
        Surface::Web | Surface::Native => {}
    }
}

/// What the app crate itself must provide; it is user code, so it is not edited.
fn print_native_guidance(ui: &Ui, app: &str) {
    ui.section(format!(
        "Generated apps/{app}/native (host crate, hooks plugin, platforms/)"
    ));
    ui.detail("The native crate hosts the app crate's pages and actions, so the app crate must be web-shaped:");
    ui.code("# apps/<app>/Cargo.toml");
    ui.code("[lib]");
    ui.code("crate-type = [\"cdylib\", \"rlib\"]");
    ui.code("[features]");
    ui.code("ssr = [\"fundamental/web\", ...]                # native host build");
    ui.code("hydrate = [\"fundamental/web-hydrate\", \"leptos/hydrate\", ...]");
    ui.code("tauri = [\"hydrate\", \"fundamental/web-hydrate-tauri\"]  # webview bundle");
    ui.detail("plus a pages/ directory, a build.rs pages scan, and `fundamental::mount_actions! {}` in lib.rs — mirror an app from `fun new --surfaces web,native`.");
    ui.note("run it with: fun dev <app> native (desktop) or --platform ios|android");
    ui.note("hand-written platform code goes under apps/<app>/native/platforms/ (see its README)");
}

/// The web setup to wire by hand: wasm library target, Leptos dependencies, `pages/`, build script.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn print_web_guidance(ui: &Ui, app: &str, tailwind: bool, domain: &DomainCrate) {
    ui.detail(format!(
        "In apps/{app}/Cargo.toml, build a wasm library alongside the binary:"
    ));
    ui.code("[lib]");
    ui.code("crate-type = [\"cdylib\", \"rlib\"]");
    ui.code("[features]");
    ui.code("# `domain/server` is the shared crate's server capability (its `#[action]`");
    ui.code("# handlers and the engines they query). The browser build enables neither, and");
    ui.code("# links neither — while using the very same model types.");
    ui.code("ssr = [\"fundamental/web\", \"domain/server\"]  # the server build (default)");
    ui.code("hydrate = [\"fundamental/web-hydrate\"]       # the browser build");
    ui.code("[dependencies]");
    ui.code(format!(
        "{}  # non-optional: compiled into both builds",
        domain.dependency(2, &[])
    ));
    ui.code("leptos = { version = \"0.8\", default-features = false, features = [\"islands\"] }");
    ui.code("serde = { version = \"1\", features = [\"derive\"] }");
    ui.code("wasm-bindgen = \"0.2\"");
    ui.code("[build-dependencies]");
    ui.code("fundamental_build = { workspace = true }");
    ui.blank();
    ui.detail("Add a build.rs that checks the pages and generates their routes and browser entry:");
    ui.code("fn main() -> std::process::ExitCode {");
    ui.code(
        "    fundamental_build::PagesBuild::run_build_script(fundamental_build::Hydration::Plain)",
    );
    ui.code("}");
    ui.blank();
    ui.detail("Declare the domain actions this app mounts — `mount_actions!` generates the");
    ui.detail("browser stubs, so no build-time source scan is involved:");
    ui.code("fundamental::mount_actions! { domain::actions::create_thing }");
    ui.blank();
    ui.detail(format!(
        "Write pages under apps/{app}/pages/ (a `page` fn per file):"
    ));
    ui.code("// pages/index.rs");
    ui.code("use fundamental::web::prelude::*;");
    ui.code("pub fn page() -> impl IntoView { view! { <h1>\"Hello\"</h1> } }");
    ui.blank();
    ui.detail(format!(
        "Then serve them from the `App` in apps/{app}/src/lib.rs (behind `#[cfg(feature = \"ssr\")]`):"
    ));
    ui.code("include!(concat!(env!(\"OUT_DIR\"), \"/pages.rs\")); // generated register_pages");
    ui.code("    fundamental::app!()");
    ui.code("        .pages(|router| register_actions(register_pages(router)?))");
    ui.code("        .actions(describe_actions)");
    ui.code("        .state(|_| Ok(()))");
    ui.note("the app serves the bundle (from disk and live-reloaded in development), the dev tools and its document shell");

    if tailwind {
        ui.blank();
        ui.section("Tailwind CSS");
        ui.detail(format!(
            "The `css_command` was added to fun.toml. Finish the setup in apps/{app}:"
        ));
        ui.code("// styles/input.css");
        ui.code("@import \"tailwindcss\";");
        ui.code("@source \"../pages\";");
        ui.blank();
        ui.detail("Link the built stylesheet from every page's head, through the app's document:");
        ui.code(".document(fundamental::web::Document::new().head(style_links))");
        ui.detail("Then `fun build`/`fun dev` runs Tailwind into dist/app.css before the bundle.");
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn workspace() -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("a project");
        fs::write(
            root.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"domain\", \"apps/www\"]\n",
        )
        .expect("the workspace");
        root
    }

    #[test]
    fn a_web_surface_brings_its_islands_crate() {
        let root = workspace();
        let wrote =
            write_islands_crate(root.path(), "www", &DomainCrate::scaffolded()).expect("written");
        assert!(wrote);
        let toml = fs::read_to_string(root.path().join("apps/www/islands/Cargo.toml"))
            .expect("the manifest");
        assert!(toml.contains("name = \"www_islands\""), "{toml}");
        assert!(root.path().join("apps/www/islands/src/lib.rs").is_file());
        let members = fs::read_to_string(root.path().join("Cargo.toml")).expect("the workspace");
        assert!(members.contains("\"apps/www/islands\""), "{members}");
    }

    #[test]
    fn an_islands_crate_already_there_is_left_alone() {
        let root = workspace();
        let islands = root.path().join("apps/www/islands");
        fs::create_dir_all(&islands).expect("the islands");
        fs::write(islands.join("Cargo.toml"), "# mine").expect("the developer's");
        let wrote = write_islands_crate(root.path(), "www", &DomainCrate::scaffolded())
            .expect("nothing to write");
        assert!(!wrote);
        assert_eq!(
            fs::read_to_string(islands.join("Cargo.toml")).expect("kept"),
            "# mine"
        );
    }
}
