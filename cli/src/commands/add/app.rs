//! `fun add app <name>`: add a new app crate to an existing project.

use super::AddAppArgs;
use crate::commands::new::generate_app_crate;
use crate::commands::scaffold::{PlanResolver, ProjectName, ProjectPlan, ScaffoldPrompts};
use crate::engine::Engine;
use crate::helpers::{DomainCrate, Manifest, Project, add_workspace_member};
use crate::ui::Ui;
use crate::{CliResult, CommandContext};
use fundamental::Validate;

/// Scaffold `apps/<name>/`, register it in `fun.toml` and the workspace, as a `fun new` app.
///
/// # Errors
/// A [`CliError`](crate::CliError) for an invalid name, no project, an existing app, an unknown
/// `--db`, a failed prompt, or an unwritable file.
pub(crate) fn run(args: AddAppArgs, ctx: CommandContext) -> CliResult<()> {
    ProjectName::new(&args.name).validate()?;

    let project = Project::discover()?;
    let mut manifest = Manifest::load(&project.manifest_path())?;

    if manifest.has_app(&args.name) {
        return Err(crate::CliError::AlreadyExists {
            what: "app".to_string(),
            name: args.name.clone(),
        });
    }

    let interactive = !args.yes && console::user_attended();
    let prompts = ScaffoldPrompts::new(ctx);
    let resolver = PlanResolver::new(&prompts, interactive);

    let surfaces = resolver.surfaces(args.surfaces.as_deref())?;
    let port = resolver.port(args.port, &surfaces, socket_port_for(&args.name));
    let mut databases = resolver.databases(args.db.as_deref(), &surfaces)?;
    let sync_store = resolver.sync_store(args.sync_store.as_deref())?;
    if let Some(engine) = &sync_store
        && !databases.contains(engine)
    {
        databases.push(engine.clone());
    }
    let tailwind = resolver.tailwind(args.tailwind, &surfaces)?;

    let app_dir = project.app_dir(&args.name);
    let domain_dir = project.resolve_domain_crate(args.domain.as_deref())?;
    let domain = DomainCrate::read(project.root(), &domain_dir)?;
    let plan = ProjectPlan::new(&args.name, app_dir.clone(), &surfaces)
        .with_app(&args.name)
        .with_socket_port(port)
        .with_databases(databases)
        .with_sync_store(sync_store)
        .with_tailwind(tailwind)
        .with_domain(domain);
    plan.domain.require(&plan.domain_features())?;

    if !confirm(&prompts, &plan, interactive)? {
        prompts.ui().status("cancelled — nothing was written");
        return Ok(());
    }

    generate_app_crate(&plan, &app_dir)?;

    manifest.add_app(&args.name, &surfaces, port)?;
    for engine in &plan.databases {
        manifest.add_database(&args.name, &plan.app_crate_name, engine)?;
    }
    manifest.save()?;

    let workspace = project.workspace_manifest_path();
    for member in plan.workspace_members() {
        add_workspace_member(&workspace, &member)?;
    }

    print_success(prompts.ui(), &plan);
    Ok(())
}

/// Interactive runs ask; non-interactive runs print the plan and proceed.
fn confirm(prompts: &ScaffoldPrompts, plan: &ProjectPlan, interactive: bool) -> CliResult<bool> {
    print_plan(prompts.ui(), plan);
    if interactive {
        return prompts.confirm("app");
    }
    prompts
        .ui()
        .note("non-interactive: creating without confirmation");
    Ok(true)
}

/// `9000` plus a stable offset from the name, so two added apps don't both default to `9000`.
fn socket_port_for(name: &str) -> u16 {
    let offset = name
        .bytes()
        .fold(0u16, |acc, b| acc.wrapping_add(u16::from(b)))
        % 1000;
    9000 + offset
}

fn print_plan(ui: &Ui, plan: &ProjectPlan) {
    ui.blank();
    ui.section("Review");
    ui.detail(format!("app       {}", plan.app_name));
    ui.detail(format!("path      {}", plan.path.display()));
    if plan.has_app() {
        ui.detail(format!("surfaces  {}", surfaces_label(plan)));
        if plan.socket {
            ui.detail(format!("socket    :{}", plan.socket_port));
        }
        if plan.web {
            ui.detail(format!(
                "tailwind  {}",
                if plan.tailwind { "yes" } else { "no" }
            ));
        }
        if !plan.databases.is_empty() {
            ui.detail(format!("database  {}", databases_label(plan)));
        }
    } else {
        ui.detail("no surfaces (edit main.rs to wire one up)");
    }
    ui.blank();
}

fn print_success(ui: &Ui, plan: &ProjectPlan) {
    ui.blank();
    ui.section(format!("Added app {}", plan.app_name));
    if plan.has_app() {
        ui.detail(format!("surfaces: {}", surfaces_label(plan)));
    } else {
        ui.detail("no surfaces (edit main.rs to wire one up)");
    }
    if !plan.databases.is_empty() {
        ui.detail(format!("database: {}", databases_label(plan)));
    }
    ui.blank();
    ui.section("Next steps");
    ui.note(format!("review apps/{}/src/main.rs", plan.app_name));
    if !plan.databases.is_empty() {
        ui.note(format!(
            "set your database connection under [apps.{}.database] in fun.toml",
            plan.app_name
        ));
    }
    ui.command("cargo build");
    ui.blank();
}

fn surfaces_label(plan: &ProjectPlan) -> String {
    plan.surfaces()
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn databases_label(plan: &ProjectPlan) -> String {
    plan.databases
        .iter()
        .map(Engine::dir_name)
        .collect::<Vec<_>>()
        .join(", ")
}
