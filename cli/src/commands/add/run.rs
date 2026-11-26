//! Dispatch for the `fun add` subcommand family.

use super::{AddArgs, AddCommand, app, authz, command, commands, migrations, seeds, surface};
use crate::{CliResult, CommandContext};

/// Dispatch to the handler for the selected thing to add.
///
/// # Errors
/// A [`CliError`](crate::CliError) from the selected subcommand handler.
pub(crate) fn run(args: AddArgs, ctx: CommandContext) -> CliResult<()> {
    match args.command {
        AddCommand::App(a) => app::run(a, ctx),
        AddCommand::Surface(a) => surface::run(a, ctx),
        AddCommand::Migrations(a) => migrations::run(a, ctx),
        AddCommand::Seeds(a) => seeds::run(a, ctx),
        AddCommand::Commands(a) => commands::run(a, ctx),
        AddCommand::Command(a) => command::run(a, ctx),
        AddCommand::Authz(a) => authz::run(a, ctx),
    }
}
