//! `fun authz`: work with the native authorization engine.

mod check;
mod client;
mod delete;
mod expand;
mod list_objects;
mod refs;
mod schema_validate;
mod write;

use clap::{Args, Subcommand};
use fundamental::authz::service::RpcError;

use crate::{CliError, CliResult, CommandContext};

pub(crate) use schema_validate::SchemaValidateArgs;

const DEFAULT_NATS_URL: &str = "nats://127.0.0.1:4222";

#[derive(Debug, Args)]
pub(crate) struct AuthzArgs {
    #[command(subcommand)]
    command: AuthzCommand,
}

#[derive(Debug, Subcommand)]
enum AuthzCommand {
    /// Validate a schema definition file (offline; no database or service needed)
    SchemaValidate(SchemaValidateArgs),
    /// Ask a running service whether a subject holds a relation on an object
    Check(check::CheckArgs),
    /// Grant a relationship tuple through a running service
    Write(write::WriteArgs),
    /// Revoke a relationship tuple through a running service
    Delete(delete::DeleteArgs),
    /// List who holds a relation on an object, via a running service
    Expand(expand::ExpandArgs),
    /// List the objects a subject holds a relation on, via a running service
    ListObjects(list_objects::ListObjectsArgs),
}

pub(crate) fn run(args: AuthzArgs, ctx: CommandContext) -> CliResult<()> {
    match args.command {
        AuthzCommand::SchemaValidate(args) => schema_validate::run(args, ctx),
        AuthzCommand::Check(args) => check::run(args, ctx),
        AuthzCommand::Write(args) => write::run(args, ctx),
        AuthzCommand::Delete(args) => delete::run(args, ctx),
        AuthzCommand::Expand(args) => expand::run(args, ctx),
        AuthzCommand::ListObjects(args) => list_objects::run(args, ctx),
    }
}

/// Keeps the service's own stable code: a backend failure is surfaced, never a silent deny.
fn service_error(error: RpcError) -> CliError {
    CliError::AuthzServiceError {
        code: error.code,
        message: error.message,
    }
}
