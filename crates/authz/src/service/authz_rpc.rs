//! [`AuthzRpc`]: the request/reply set gathering every authorization RPC.

use fundamental_nats::RpcSet;

use super::check_rpc::{CheckRpcReply, CheckRpcRequest};
use super::delete_rpc::{DeleteRpcReply, DeleteRpcRequest};
use super::expand_rpc::{ExpandRpcReply, ExpandRpcRequest};
use super::list_objects_rpc::{ListObjectsRpcReply, ListObjectsRpcRequest};
use super::write_rpc::{WriteRpcReply, WriteRpcRequest};

/// The closed set of authorization RPCs — one request/reply pair per
/// [`Authorizer`](crate::authorizer::Authorizer) method that crosses the wire.
///
/// [`RpcSet`] derives each request and reply subject (`authz.rpc.check`,
/// `authz.rpc.check.reply`, …), so no wire literal is hand-typed. Never constructed.
#[derive(RpcSet)]
pub enum AuthzRpc {
    /// May a subject perform a relation on an object?
    Check(CheckRpcRequest, CheckRpcReply),
    /// Who holds a relation on an object?
    Expand(ExpandRpcRequest, ExpandRpcReply),
    /// Which objects does a subject hold a relation on?
    ListObjects(ListObjectsRpcRequest, ListObjectsRpcReply),
    /// Upsert relationship tuples.
    Write(WriteRpcRequest, WriteRpcReply),
    /// Delete relationship tuples.
    Delete(DeleteRpcRequest, DeleteRpcReply),
}

#[cfg(test)]
mod tests {
    use fundamental_nats::RpcSet as _;

    use super::*;

    #[test]
    fn every_rpc_has_a_request_and_reply_subject() {
        assert_eq!(
            AuthzRpc::REQUEST_SUBJECTS,
            [
                "authz.rpc.check",
                "authz.rpc.expand",
                "authz.rpc.list.objects",
                "authz.rpc.write",
                "authz.rpc.delete",
            ]
        );
        assert!(
            AuthzRpc::REPLY_SUBJECTS
                .iter()
                .all(|subject| subject.ends_with(".reply"))
        );
    }
}
