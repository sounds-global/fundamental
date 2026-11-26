//! `fun migrate create <name>`: create a timestamped migration pair.

use super::MigrateCreateArgs;
use super::timestamp::migration_timestamp;
use crate::engine::Engine;
use crate::helpers::{Project, down_stub, up_stub, write_file};
use crate::{CliResult, CommandContext};

/// Write a `<timestamp>_<name>.up/.down.<ext>` pair under `apps/<app>/migrations/<engine>/`.
///
/// # Errors
/// A [`CliError`](crate::CliError) if the project cannot be found or a file cannot be written.
pub(crate) fn run(args: MigrateCreateArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let engine = Engine::parse(&args.engine);
    let engine_dir = project
        .app_dir(&args.app)
        .join("migrations")
        .join(engine.dir_name());

    let ext = engine.extension();
    let stem = format!("{}_{}", migration_timestamp(), args.name);
    let up = engine_dir.join(format!("{stem}.up.{ext}"));
    let down = engine_dir.join(format!("{stem}.down.{ext}"));

    write_file(&up, up_stub())?;
    write_file(&down, down_stub())?;

    let ui = ctx.ui();
    ui.blank();
    ui.section(format!("Created migration {stem}"));
    ui.detail(format!(
        "apps/{}/migrations/{}/{stem}.up.{ext}",
        args.app,
        engine.dir_name()
    ));
    ui.detail(format!(
        "apps/{}/migrations/{}/{stem}.down.{ext}",
        args.app,
        engine.dir_name()
    ));
    ui.blank();
    Ok(())
}
