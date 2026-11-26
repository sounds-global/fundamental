//! `fun authz list-objects`: the objects of a namespace a subject holds a relation on.

use clap::Args;
use fundamental::authz::schema::{NamespaceName, RelationName};
use fundamental::authz::service::{ListObjectsRpcReply, ListObjectsRpcRequest};
use fundamental::authz::tuple::CaveatContext;
use fundamental::authz::zookie::Consistency;

use super::client::request;
use super::refs::parse_subject;
use super::{DEFAULT_NATS_URL, service_error};
use crate::{CliResult, CommandContext};

#[derive(Debug, Args)]
pub(crate) struct ListObjectsArgs {
    /// The subject, as `namespace:id` or `namespace:id#relation`.
    pub subject: String,
    /// The object namespace to search (e.g. `document`).
    pub namespace: String,
    /// The relation the subject must hold on each returned object (e.g. `viewer`).
    pub relation: String,
    /// The maximum number of objects to return.
    #[arg(long, default_value_t = 100)]
    pub limit: usize,
    /// URL of the running authorization service.
    #[arg(long, default_value = DEFAULT_NATS_URL)]
    pub nats_url: String,
}

pub(crate) fn run(args: ListObjectsArgs, ctx: CommandContext) -> CliResult<()> {
    let request_body = ListObjectsRpcRequest {
        subject: parse_subject(&args.subject)?,
        object_namespace: NamespaceName::new(args.namespace.as_str())?,
        relation: RelationName::new(args.relation.as_str())?,
        context: CaveatContext::new(),
        consistency: Consistency::default(),
        limit: args.limit,
    };

    let reply: ListObjectsRpcReply = request(&args.nats_url, request_body)?;
    let response = reply.outcome.map_err(service_error)?;

    let ui = ctx.ui();
    ui.section(format!("{} object(s)", response.objects.len()));
    for object in &response.objects {
        ui.detail(object.to_string());
    }
    ui.detail(format!("evaluated at {}", response.evaluated_at));
    Ok(())
}
