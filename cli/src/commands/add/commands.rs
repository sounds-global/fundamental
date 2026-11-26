//! `fun add commands`: scaffold a Rust `commands` binary with a sample `#[command]`.

use tracing::instrument;

use super::AddCommandsArgs;
use crate::helpers::{CrateManifest, Project, write_file};
use crate::{CliResult, CommandContext};

/// Create `apps/<app>/src/bin/commands.rs` and enable the app's `command` feature.
///
/// # Errors
/// A [`CliError`](crate::CliError) for a missing project or app, an unwritable file, or no
/// `fundamental` dependency.
#[instrument(skip(ctx), fields(app = %args.app), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn run(args: AddCommandsArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let app_dir = project.app_dir(&args.app);

    let bin_path = app_dir.join("src").join("bin").join("commands.rs");
    write_file(&bin_path, COMMANDS_BIN)?;

    let cargo_path = app_dir.join("Cargo.toml");
    let mut manifest = CrateManifest::load(&cargo_path)?;
    manifest.ensure_fundamental_feature("command")?;
    manifest.save()?;

    let ui = ctx.ui();
    ui.blank();
    ui.section(format!("Created commands for {}", args.app));
    ui.detail(format!("apps/{}/src/bin/commands.rs", args.app));
    ui.blank();
    ui.section("Next steps");
    ui.command("fun run --list");
    ui.command("fun run app:hello");
    ui.note("add a #[command] fn and list it in commands_main! (see the file)");
    ui.blank();
    Ok(())
}

const COMMANDS_BIN: &str = r#"//! Project commands, run with `fun run <namespace>:<name>`; list each in `commands_main!`.

use fundamental::command;
use fundamental::command::{CommandArgs, CommandError};

/// Your application's context, built once in `commands_main!`.
struct AppContext;

/// A sample command: `fun run app:hello`.
#[command(namespace = "app", description = "A sample command")]
async fn hello(ctx: &AppContext, args: CommandArgs) -> Result<(), CommandError> {
    let _ = (ctx, args);
    println!("hello from fun run app:hello");
    Ok(())
}

fundamental::commands_main! {
    context = AppContext,
    commands = [hello],
}
"#;
