//! The `check` RPC: its request, reply, and `#[respond]` handler.

use fundamental_macros::respond;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use super::authz_service::AuthzService;
use super::authz_workers::AuthzWorkers;
use super::rpc_error::RpcError;
use crate::authorizer::CheckResponse;
use crate::schema::RelationName;
use crate::tuple::{CaveatContext, ObjectRef, SubjectRef};
use crate::zookie::Consistency;

/// A remote permission check: may `subject` perform `relation` on `object`?
///
/// The wire form of [`Authorizer::check`](crate::authorizer::Authorizer::check); every name
/// field deserializes through its validating newtype.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckRpcRequest {
    /// The object the permission is on.
    pub object: ObjectRef,
    /// The relation being checked.
    pub relation: RelationName,
    /// The subject whose access is in question.
    pub subject: SubjectRef,
    /// Must be empty; a request that binds any parameter is refused with `AUTHZ_019`.
    #[serde(default)]
    pub context: CaveatContext,
    /// The freshness the caller requires.
    #[serde(default)]
    pub consistency: Consistency,
}

/// The reply to a [`CheckRpcRequest`]: the decision, or the [`RpcError`] that stopped it, so
/// "not allowed" and "could not decide" stay apart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckRpcReply {
    /// The decision, or the error that prevented one.
    pub outcome: Result<CheckResponse, RpcError>,
}

/// Serves the `check` RPC.
#[respond(queue = AuthzWorkers)]
#[instrument(skip(service, request), fields(
    object = %request.object,
    relation = %request.relation,
    subject = %request.subject,
))]
pub async fn check_rpc(service: &AuthzService, request: CheckRpcRequest) -> CheckRpcReply {
    let CheckRpcRequest {
        object,
        relation,
        subject,
        context,
        consistency,
    } = request;
    let outcome = service
        .authorizer()
        .check(object, relation, subject, context, consistency)
        .await;
    CheckRpcReply {
        outcome: outcome.map_err(|err| RpcError::boundary("check", &err)),
    }
}
