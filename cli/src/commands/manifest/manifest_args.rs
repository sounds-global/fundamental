//! [`ManifestArgs`]: the arguments of `fun manifest`, and the dispatch to each action.

use clap::{Args, Subcommand};

use super::show::{self, ShowArgs};
use crate::{CliResult, CommandContext};

#[derive(Debug, Args)]
pub(crate) struct ManifestArgs {
    #[command(subcommand)]
    command: ManifestCommand,
}

#[derive(Debug, Subcommand)]
enum ManifestCommand {
    /// Print a key's value and the file and line it is written at
    Show(ShowArgs),
}

/// Dispatch `fun manifest` to its action.
///
/// # Errors
/// Whatever the action returns.
pub(crate) fn run(args: ManifestArgs, ctx: CommandContext) -> CliResult<()> {
    match args.command {
        ManifestCommand::Show(args) => show::run(&args, &ctx),
    }
}
