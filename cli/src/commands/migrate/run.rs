//! Dispatch for the `fun migrate` subcommand family.

use super::generate::Writes;
use super::{MigrateArgs, MigrateCommand, create, generate, list, rebase, resolve, spawn, verify};
use crate::{CliResult, CommandContext};

/// Dispatch to the selected migration operation.
///
/// # Errors
/// A [`CliError`](crate::CliError) from the selected subcommand handler.
pub(crate) fn run(args: MigrateArgs, ctx: CommandContext) -> CliResult<()> {
    match args.command {
        MigrateCommand::Create(a) => create::run(a, ctx),
        MigrateCommand::List(a) => list::run(a, ctx),
        MigrateCommand::Check(a) => {
            spawn::run(a.app.as_deref(), a.bin.as_deref(), &["check"], &ctx)
        }
        MigrateCommand::Apply(a) => {
            spawn::run(a.app.as_deref(), a.bin.as_deref(), &["apply"], &ctx)
        }
        MigrateCommand::Status(a) => {
            spawn::run(a.app.as_deref(), a.bin.as_deref(), &["status"], &ctx)
        }
        MigrateCommand::Verify(a) => verify::run(a, ctx),
        MigrateCommand::Resolve(a) => resolve::run(&a, &ctx),
        MigrateCommand::Generate(a) => generate::run(a.package, &Writes::Next(a.name), ctx),
        MigrateCommand::Regenerate(a) => generate::run(a.package, &Writes::Regenerate, ctx),
        MigrateCommand::Rebase(a) => rebase::run(a, ctx),
    }
}
