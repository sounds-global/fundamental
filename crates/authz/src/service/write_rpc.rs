//! The `write` RPC: its request, reply, and `#[respond]` handler.

use fundamental_macros::respond;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use super::authz_service::AuthzService;
use super::authz_workers::AuthzWorkers;
use super::rpc_error::RpcError;
use crate::tuple::RelationTuple;
use crate::zookie::Zookie;

/// A remote write: upsert relationship tuples.
///
/// The wire form of [`Authorizer::write`](crate::authorizer::Authorizer::write).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WriteRpcRequest {
    /// The tuples to write.
    pub tuples: Vec<RelationTuple>,
}

/// The reply to a [`WriteRpcRequest`]: the commit [`Zookie`], or the error.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WriteRpcReply {
    /// The commit token, or the error that prevented the write.
    pub outcome: Result<Zookie, RpcError>,
}

/// Serves the `write` RPC.
#[respond(queue = AuthzWorkers)]
#[instrument(skip(service, request), fields(tuples = request.tuples.len()))]
pub async fn write_rpc(service: &AuthzService, request: WriteRpcRequest) -> WriteRpcReply {
    let outcome = service.authorizer().write(&request.tuples).await;
    WriteRpcReply {
        outcome: outcome.map_err(|err| RpcError::boundary("write", &err)),
    }
}
