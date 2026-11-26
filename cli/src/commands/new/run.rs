//! Entry point for `fun new`: resolve every choice, confirm the plan, and generate the workspace.

use std::path::PathBuf;

use super::NewArgs;
use super::generate::generate;
use crate::commands::scaffold::{DEFAULT_SOCKET_PORT, PlanResolver, ProjectPlan, ScaffoldPrompts};
use crate::engine::Engine;
use crate::ui::Ui;
use crate::{CliError, CliResult, CommandContext};

/// Resolve every choice, confirm the plan, and scaffold the project.
///
/// # Errors
/// A [`CliError`] for an invalid or missing name, an existing directory, an unknown `--db`, a
/// failed prompt, or unwritable files.
pub(crate) fn run(args: NewArgs, ctx: CommandContext) -> CliResult<()> {
    let prompts = ScaffoldPrompts::new(ctx);
    prompts.intro();

    // Prompt only when attended, so a scripted run never blocks.
    let interactive = !args.yes && console::user_attended();
    let resolver = PlanResolver::new(&prompts, interactive);

    let name = resolver.name(args.name.as_deref())?;
    let path = args.path.clone().unwrap_or_else(|| PathBuf::from(&name));
    if path.exists() {
        return Err(CliError::DirectoryExists {
            path: path.display().to_string(),
        });
    }

    let surfaces = resolver.surfaces(args.surfaces.as_deref())?;
    let app = resolver.app(args.app.as_deref(), &surfaces)?;
    let port = resolver.port(args.port, &surfaces, DEFAULT_SOCKET_PORT);
    let mut databases = resolver.databases(args.db.as_deref(), &surfaces)?;
    let sync_store = resolver.sync_store(args.sync_store.as_deref())?;
    // The slice-store backend must be among the wired databases.
    if let Some(engine) = &sync_store
        && !databases.contains(engine)
    {
        databases.push(engine.clone());
    }
    let tailwind = resolver.tailwind(args.tailwind, &surfaces)?;

    let plan = ProjectPlan::new(&name, path, &surfaces)
        .with_app(&app)
        .with_socket_port(port)
        .with_databases(databases)
        .with_sync_store(sync_store)
        .with_tailwind(tailwind);

    if !confirm(&prompts, &plan, interactive)? {
        prompts.ui().status("cancelled — nothing was written");
        return Ok(());
    }

    generate(&plan)?;
    print_success(prompts.ui(), &plan);
    Ok(())
}

/// Interactive runs ask; non-interactive runs print the plan and proceed.
fn confirm(prompts: &ScaffoldPrompts, plan: &ProjectPlan, interactive: bool) -> CliResult<bool> {
    print_plan(prompts.ui(), plan);
    if interactive {
        return prompts.confirm("project");
    }
    prompts
        .ui()
        .note("non-interactive: creating without confirmation");
    Ok(true)
}

fn print_plan(ui: &Ui, plan: &ProjectPlan) {
    ui.blank();
    ui.section("Review");
    ui.detail(format!("project   {}", plan.name));
    ui.detail(format!("path      {}", plan.path.display()));
    ui.detail("domain    domain/ (shared model crate)");
    if plan.has_app() {
        ui.detail(format!(
            "app       {} · {}",
            plan.app_name,
            surfaces_label(plan)
        ));
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
        ui.detail("library   no runtime surfaces");
    }
    ui.blank();
}

fn surfaces_label(plan: &ProjectPlan) -> String {
    let mut names = Vec::new();
    if plan.worker {
        names.push("worker");
    }
    if plan.socket {
        names.push("socket");
    }
    if plan.api {
        names.push("api");
    }
    if plan.web {
        names.push("web");
    }
    if plan.sync {
        names.push("sync");
    }
    names.join(", ")
}

fn databases_label(plan: &ProjectPlan) -> String {
    plan.databases
        .iter()
        .map(Engine::dir_name)
        .collect::<Vec<_>>()
        .join(", ")
}

fn print_success(ui: &Ui, plan: &ProjectPlan) {
    ui.blank();
    ui.section(format!("Created {}", plan.name));
    ui.detail(plan.path.display());
    ui.detail("domain (shared model crate)");
    if plan.has_app() {
        ui.detail(format!("app {} · {}", plan.app_name, surfaces_label(plan)));
    } else {
        ui.detail("library only — no runtime surfaces");
    }

    ui.blank();
    ui.section("Next steps");
    ui.command(format!("cd {}", plan.path.display()));
    ui.command("cargo run");
    ui.note(
        "`fundamental` is pulled from git over SSH — ensure your key has repo access (or patch a local path)",
    );
    if plan.worker {
        ui.note(format!(
            "configure NATS under [apps.{}] in fun.toml",
            plan.app_name
        ));
    }
    if !plan.databases.is_empty() {
        ui.note(format!(
            "set your database connection under [apps.{}.database] in fun.toml",
            plan.app_name
        ));
    }
    ui.blank();
}
