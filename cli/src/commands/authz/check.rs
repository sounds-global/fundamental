//! `fun authz check`: ask a running service whether a subject holds a relation on an object.

use clap::Args;
use fundamental::authz::schema::RelationName;
use fundamental::authz::service::{CheckRpcReply, CheckRpcRequest};
use fundamental::authz::tuple::CaveatContext;
use fundamental::authz::zookie::Consistency;

use super::client::request;
use super::refs::{parse_object, parse_subject};
use super::{DEFAULT_NATS_URL, service_error};
use crate::{CliResult, CommandContext};

/// Arguments accepted by `fun authz check`.
#[derive(Debug, Args)]
pub(crate) struct CheckArgs {
    /// The object, as `namespace:id` (e.g. `document:readme`).
    pub object: String,
    /// The relation to check (e.g. `viewer`).
    pub relation: String,
    /// The subject, as `namespace:id` or `namespace:id#relation` (e.g. `user:alice`, `group:eng#member`)
    pub subject: String,
    /// URL of the running authorization service.
    #[arg(long, default_value = DEFAULT_NATS_URL)]
    pub nats_url: String,
}

pub(crate) fn run(args: CheckArgs, ctx: CommandContext) -> CliResult<()> {
    let request_body = CheckRpcRequest {
        object: parse_object(&args.object)?,
        relation: RelationName::new(args.relation.as_str())?,
        subject: parse_subject(&args.subject)?,
        context: CaveatContext::new(),
        consistency: Consistency::default(),
    };

    let reply: CheckRpcReply = request(&args.nats_url, request_body)?;
    let response = reply.outcome.map_err(service_error)?;

    let ui = ctx.ui();
    ui.section(if response.allowed {
        "ALLOWED"
    } else {
        "DENIED"
    });
    ui.detail(format!("evaluated at {}", response.evaluated_at));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Probe {
        #[command(flatten)]
        args: CheckArgs,
    }

    #[test]
    fn takes_object_relation_subject_positionally_with_a_default_url() {
        let probe = Probe::try_parse_from(["fun", "document:readme", "viewer", "user:alice"])
            .expect("three positional args are valid");
        assert_eq!(probe.args.object, "document:readme");
        assert_eq!(probe.args.relation, "viewer");
        assert_eq!(probe.args.subject, "user:alice");
        assert_eq!(probe.args.nats_url, DEFAULT_NATS_URL);
    }

    #[test]
    fn overrides_the_service_url() {
        let probe = Probe::try_parse_from([
            "fun",
            "document:readme",
            "viewer",
            "user:alice",
            "--nats-url",
            "nats://example:4222",
        ])
        .expect("an explicit url is valid");
        assert_eq!(probe.args.nats_url, "nats://example:4222");
    }

    #[test]
    fn requires_all_three_references() {
        assert!(Probe::try_parse_from(["fun", "document:readme", "viewer"]).is_err());
    }
}
