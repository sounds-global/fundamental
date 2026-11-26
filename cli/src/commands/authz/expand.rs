//! `fun authz expand`: list who holds a relation on an object, via a running service.

use clap::Args;
use fundamental::authz::schema::RelationName;
use fundamental::authz::service::{ExpandRpcReply, ExpandRpcRequest};
use fundamental::authz::zookie::Consistency;

use super::client::request;
use super::refs::parse_object;
use super::{DEFAULT_NATS_URL, service_error};
use crate::{CliResult, CommandContext};

#[derive(Debug, Args)]
pub(crate) struct ExpandArgs {
    /// The object, as `namespace:id` (e.g. `document:readme`).
    pub object: String,
    /// The relation to expand (e.g. `viewer`).
    pub relation: String,
    /// URL of the running authorization service.
    #[arg(long, default_value = DEFAULT_NATS_URL)]
    pub nats_url: String,
}

pub(crate) fn run(args: ExpandArgs, ctx: CommandContext) -> CliResult<()> {
    let request_body = ExpandRpcRequest {
        object: parse_object(&args.object)?,
        relation: RelationName::new(args.relation.as_str())?,
        consistency: Consistency::default(),
    };

    let reply: ExpandRpcReply = request(&args.nats_url, request_body)?;
    let response = reply.outcome.map_err(service_error)?;

    // Flat: intersection and exclusion are not honoured; `fun authz check` is authoritative.
    let subjects = response.tree.all_subjects();
    let ui = ctx.ui();
    ui.section(format!("{} subject(s)", subjects.len()));
    for subject in &subjects {
        ui.detail(subject.to_string());
    }
    ui.detail(format!("evaluated at {}", response.evaluated_at));
    Ok(())
}
