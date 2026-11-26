//! The `expand` RPC: its request, reply, and `#[respond]` handler.

use fundamental_macros::respond;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use super::authz_service::AuthzService;
use super::authz_workers::AuthzWorkers;
use super::rpc_error::RpcError;
use crate::authorizer::ExpandResponse;
use crate::schema::RelationName;
use crate::tuple::ObjectRef;
use crate::zookie::Consistency;

/// A remote expansion: who holds `relation` on `object`, as a userset tree?
///
/// The wire form of [`Authorizer::expand`](crate::authorizer::Authorizer::expand).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpandRpcRequest {
    /// The object whose relation is expanded.
    pub object: ObjectRef,
    /// The relation to expand.
    pub relation: RelationName,
    /// The freshness the caller requires.
    #[serde(default)]
    pub consistency: Consistency,
}

/// The reply to an [`ExpandRpcRequest`]: the userset tree, or the error.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpandRpcReply {
    /// The expansion, or the error that prevented one.
    pub outcome: Result<ExpandResponse, RpcError>,
}

/// Serves the `expand` RPC.
#[respond(queue = AuthzWorkers)]
#[instrument(skip(service, request), fields(
    object = %request.object,
    relation = %request.relation,
))]
pub async fn expand_rpc(service: &AuthzService, request: ExpandRpcRequest) -> ExpandRpcReply {
    let ExpandRpcRequest {
        object,
        relation,
        consistency,
    } = request;
    let outcome = service
        .authorizer()
        .expand(object, relation, consistency)
        .await;
    ExpandRpcReply {
        outcome: outcome.map_err(|err| RpcError::boundary("expand", &err)),
    }
}
