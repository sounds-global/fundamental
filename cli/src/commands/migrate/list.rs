//! `fun migrate list`: list the migrations discovered for an app.

use fundamental::build::discover_migrations;

use super::MigrateListArgs;
use crate::helpers::Project;
use crate::{CliError, CliResult, CommandContext};

/// Print every migration for the app: its engine, name, and whether a down script exists.
///
/// # Errors
/// A [`CliError`] if the project cannot be found or the migrations tree cannot be read.
pub(crate) fn run(args: MigrateListArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let app_dir = project.app_dir(&args.app);

    let migrations = discover_migrations(&app_dir).map_err(|e| CliError::Io {
        path: app_dir.join("migrations").display().to_string(),
        reason: e.to_string(),
    })?;

    let ui = ctx.ui();
    if migrations.is_empty() {
        ui.status(format!("no migrations found for app {}", args.app));
        return Ok(());
    }

    let rows: Vec<Vec<String>> = migrations
        .iter()
        .map(|m| {
            let down = if m.down.is_some() {
                ui.ok("yes")
            } else {
                ui.warn("no")
            };
            vec![m.engine.clone(), m.name.clone(), down]
        })
        .collect();

    ui.blank();
    ui.table(&["ENGINE", "NAME", "DOWN"], &rows);
    ui.blank();
    Ok(())
}
