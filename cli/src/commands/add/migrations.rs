//! `fun add migrations`: create a migrations directory with a sample migration.

use std::fs;

use fundamental::build::discover_migrations;

use super::AddMigrationsArgs;
use crate::engine::Engine;
use crate::helpers::{Project, down_stub, up_stub, write_file};
use crate::{CliError, CliResult, CommandContext};

/// Ensure `apps/<app>/migrations/<engine>/` exists, with a `0001_init` pair when it holds none.
///
/// # Errors
/// A [`CliError`] if the project cannot be found, migrations cannot be read, or a file cannot be written.
pub(crate) fn run(args: AddMigrationsArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let app_dir = project.app_dir(&args.app);
    let engine = Engine::parse(&args.engine);
    let engine_dir = app_dir.join("migrations").join(engine.dir_name());

    fs::create_dir_all(&engine_dir).map_err(|e| CliError::Io {
        path: engine_dir.display().to_string(),
        reason: e.to_string(),
    })?;

    let existing = discover_migrations(&app_dir).map_err(map_build_err)?;
    let has_for_engine = existing.iter().any(|m| m.engine == engine.dir_name());

    let ui = ctx.ui();
    if has_for_engine {
        ui.status(format!(
            "migrations/{} already exists for app {}; left untouched",
            engine.dir_name(),
            args.app,
        ));
        return Ok(());
    }

    let ext = engine.extension();
    let up = engine_dir.join(format!("0001_init.up.{ext}"));
    let down = engine_dir.join(format!("0001_init.down.{ext}"));
    write_file(&up, up_stub())?;
    write_file(&down, down_stub())?;

    ui.blank();
    ui.section(format!(
        "Created migrations for {} ({})",
        args.app,
        engine.dir_name()
    ));
    ui.detail(format!(
        "apps/{}/migrations/{}/0001_init.up.{ext}",
        args.app,
        engine.dir_name()
    ));
    ui.blank();
    Ok(())
}

fn map_build_err(err: impl std::fmt::Display) -> CliError {
    CliError::Io {
        path: "migrations".to_string(),
        reason: err.to_string(),
    }
}
