//! The `list_objects` RPC: its request, reply, and `#[respond]` handler.

use fundamental_macros::respond;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use super::authz_service::AuthzService;
use super::authz_workers::AuthzWorkers;
use super::rpc_error::RpcError;
use crate::authorizer::ListObjectsResponse;
use crate::schema::{NamespaceName, RelationName};
use crate::tuple::{CaveatContext, SubjectRef};
use crate::zookie::Consistency;

/// A remote reverse lookup, the wire form of
/// [`Authorizer::list_objects`](crate::authorizer::Authorizer::list_objects).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListObjectsRpcRequest {
    /// The subject whose objects are listed.
    pub subject: SubjectRef,
    /// The namespace of the objects to return.
    pub object_namespace: NamespaceName,
    /// The relation the subject must hold on each returned object.
    pub relation: RelationName,
    /// Must be empty; a request that binds any parameter is refused with `AUTHZ_019`.
    #[serde(default)]
    pub context: CaveatContext,
    /// The freshness the caller requires.
    #[serde(default)]
    pub consistency: Consistency,
    /// The maximum number of objects to return.
    pub limit: usize,
}

/// The reply to a [`ListObjectsRpcRequest`]: the objects, or the error.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListObjectsRpcReply {
    /// The matching objects, or the error that prevented listing them.
    pub outcome: Result<ListObjectsResponse, RpcError>,
}

/// Serves the `list_objects` RPC.
#[respond(queue = AuthzWorkers)]
#[instrument(skip(service, request), fields(
    subject = %request.subject,
    object_namespace = %request.object_namespace,
    relation = %request.relation,
    limit = request.limit,
))]
pub async fn list_objects_rpc(
    service: &AuthzService,
    request: ListObjectsRpcRequest,
) -> ListObjectsRpcReply {
    let ListObjectsRpcRequest {
        subject,
        object_namespace,
        relation,
        context,
        consistency,
        limit,
    } = request;
    let outcome = service
        .authorizer()
        .list_objects(
            subject,
            object_namespace,
            relation,
            context,
            consistency,
            limit,
        )
        .await;
    ListObjectsRpcReply {
        outcome: outcome.map_err(|err| RpcError::boundary("list_objects", &err)),
    }
}
