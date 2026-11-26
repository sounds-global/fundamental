//! The standalone authorization service: the same [`Authorizer`](crate::authorizer::Authorizer)
//! on NATS, served by [`AuthzService`] on the [`AuthzWorkers`] queue. A backend failure rides
//! back as an [`RpcError`], never as a denial.

mod authz_events;
mod authz_rpc;
mod authz_service;
mod authz_workers;
mod check_rpc;
mod delete_rpc;
mod expand_rpc;
mod list_objects_rpc;
mod rpc_error;
mod schema_changed;
mod tuple_changed;
mod write_rpc;

pub use authz_events::AuthzEvents;
pub use authz_rpc::AuthzRpc;
pub use authz_service::AuthzService;
// Native-only, as the descriptor accessors `#[respond]` emits are.
#[cfg(not(target_arch = "wasm32"))]
pub use authz_service::describe_responders;
pub use authz_workers::AuthzWorkers;
pub use check_rpc::{CheckRpcReply, CheckRpcRequest, check_rpc, check_rpc_responder};
pub use delete_rpc::{DeleteRpcReply, DeleteRpcRequest, delete_rpc, delete_rpc_responder};
pub use expand_rpc::{ExpandRpcReply, ExpandRpcRequest, expand_rpc, expand_rpc_responder};
pub use list_objects_rpc::{
    ListObjectsRpcReply, ListObjectsRpcRequest, list_objects_rpc, list_objects_rpc_responder,
};
pub use rpc_error::RpcError;
pub use schema_changed::SchemaChanged;
pub use tuple_changed::TupleChanged;
pub use write_rpc::{WriteRpcReply, WriteRpcRequest, write_rpc, write_rpc_responder};
