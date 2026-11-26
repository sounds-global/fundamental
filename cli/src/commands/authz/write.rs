//! `fun authz write`: upsert a relationship tuple through a running service.

use clap::Args;
use fundamental::authz::schema::RelationName;
use fundamental::authz::service::{WriteRpcReply, WriteRpcRequest};
use fundamental::authz::tuple::RelationTuple;

use super::client::request;
use super::refs::{parse_object, parse_subject};
use super::{DEFAULT_NATS_URL, service_error};
use crate::{CliResult, CommandContext};

#[derive(Debug, Args)]
pub(crate) struct WriteArgs {
    /// The object, as `namespace:id` (e.g. `document:readme`).
    pub object: String,
    /// The relation to grant (e.g. `owner`).
    pub relation: String,
    /// The subject, as `namespace:id` or `namespace:id#relation`.
    pub subject: String,
    /// URL of the running authorization service.
    #[arg(long, default_value = DEFAULT_NATS_URL)]
    pub nats_url: String,
}

pub(crate) fn run(args: WriteArgs, ctx: CommandContext) -> CliResult<()> {
    let tuple = RelationTuple::new(
        parse_object(&args.object)?,
        RelationName::new(args.relation.as_str())?,
        parse_subject(&args.subject)?,
    );
    let request_body = WriteRpcRequest {
        tuples: vec![tuple],
    };

    let reply: WriteRpcReply = request(&args.nats_url, request_body)?;
    let zookie = reply.outcome.map_err(service_error)?;

    let ui = ctx.ui();
    ui.section("Tuple written");
    ui.detail(format!("committed at {zookie}"));
    Ok(())
}
