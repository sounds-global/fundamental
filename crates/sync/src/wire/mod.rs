//! The sync wire protocol: serde DTOs exchanged between a device and the server.
//!
//! Separate from the engine's domain types, which carry no serde derive; each request carries a
//! [`PROTOCOL_VERSION`] and a `request_id` its response echoes.

mod auth_ack;
mod auth_request;
mod canonical;
mod device_ack;
mod enroll_request;
mod error_reply;
mod hosted_rows;
mod protocol_version;
mod revoke_request;
mod rotate_request;
mod slice_change;
mod slice_cursor;
mod slice_pull_request;
mod slice_pull_response;
mod slice_subscribe_ack;
mod slice_subscribe_request;
mod slice_upsert_request;
mod sync_action;
mod upsert_ack;
mod vault_auth_request;
mod vault_challenge_request;
mod vault_challenge_response;
mod versioned_request;
mod wire_meta;
mod wire_record;

pub use auth_ack::AuthAck;
pub use auth_request::AuthRequest;
pub use canonical::canonical_bytes;
pub use device_ack::DeviceAck;
pub use enroll_request::EnrollRequest;
pub use error_reply::{ErrorReply, WireError};
pub use hosted_rows::{HostedRows, HostedValue};
pub use protocol_version::{PROTOCOL_VERSION, SUPPORTED_VERSIONS};
pub use revoke_request::RevokeRequest;
pub use rotate_request::RotateRequest;
pub use slice_change::SliceChange;
pub use slice_cursor::SliceCursor;
pub use slice_pull_request::SlicePullRequest;
pub use slice_pull_response::SlicePullResponse;
pub use slice_subscribe_ack::SliceSubscribeAck;
pub use slice_subscribe_request::SliceSubscribeRequest;
pub use slice_upsert_request::SliceUpsertRequest;
pub use upsert_ack::UpsertAck;
pub use vault_auth_request::VaultAuthRequest;
pub use vault_challenge_request::VaultChallengeRequest;
pub use vault_challenge_response::VaultChallengeResponse;
pub use versioned_request::VersionedRequest;
pub use wire_meta::WireMeta;
pub use wire_record::WireRecord;

pub mod action {
    //! Canonical action strings for the sync wire protocol.
    pub use super::sync_action::{
        AUTH, DEVICE_ENROLL, DEVICE_REVOKE, DEVICE_ROTATE, SLICE_CHANGE, SLICE_PULL,
        SLICE_SUBSCRIBE, SLICE_UPSERT, VAULT_AUTH, VAULT_CHALLENGE,
    };
}
