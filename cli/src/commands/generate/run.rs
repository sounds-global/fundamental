//! Dispatch for the `fun generate` subcommand family.

use super::{GenerateArgs, GenerateCommand, action, model, slice};
use crate::{CliResult, CommandContext};

/// Dispatch to the handler for the selected generator.
///
/// # Errors
/// A [`CliError`](crate::CliError) from the selected subcommand handler.
pub(crate) fn run(args: GenerateArgs, ctx: CommandContext) -> CliResult<()> {
    match args.command {
        GenerateCommand::Model(a) => model::run(a, ctx),
        GenerateCommand::Action(a) => action::run(a, ctx),
        GenerateCommand::Slice(a) => slice::run(a, ctx),
    }
}
