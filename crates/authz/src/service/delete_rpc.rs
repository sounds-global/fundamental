//! The `delete` RPC: its request, reply, and `#[respond]` handler.

use fundamental_macros::respond;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use super::authz_service::AuthzService;
use super::authz_workers::AuthzWorkers;
use super::rpc_error::RpcError;
use crate::tuple::RelationTuple;
use crate::zookie::Zookie;

/// A remote delete: remove relationship tuples by identity.
///
/// The wire form of [`Authorizer::delete`](crate::authorizer::Authorizer::delete).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeleteRpcRequest {
    /// The tuples to delete, matched by identity.
    pub tuples: Vec<RelationTuple>,
}

/// The reply to a [`DeleteRpcRequest`]: the commit [`Zookie`], or the error.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeleteRpcReply {
    /// The commit token, or the error that prevented the delete.
    pub outcome: Result<Zookie, RpcError>,
}

/// Serves the `delete` RPC.
#[respond(queue = AuthzWorkers)]
#[instrument(skip(service, request), fields(tuples = request.tuples.len()))]
pub async fn delete_rpc(service: &AuthzService, request: DeleteRpcRequest) -> DeleteRpcReply {
    let outcome = service.authorizer().delete(&request.tuples).await;
    DeleteRpcReply {
        outcome: outcome.map_err(|err| RpcError::boundary("delete", &err)),
    }
}
