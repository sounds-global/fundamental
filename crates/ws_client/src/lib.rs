//! Device-side WebSocket transport client for the [Fundamental](https://docs.rs/fundamental)
//! framework: [`WsRemoteSource`] implements the sync engine's
//! [`RemoteSource`](fundamental_sync::RemoteSource) over one socket, natively and in the browser.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod connect_failure;
mod connection;
mod connection_status;
mod connection_watch;
mod platform;
mod reconnect_hint;
mod reconnect_max_attempts_is_removed;
mod reconnect_schedule;
mod status_board;
mod sync_auth;
mod sync_client;
mod sync_client_config;
mod transport_error;
mod unsigned;
mod wait_cause;
mod ws_remote_source;

pub use connection_status::ConnectionStatus;
pub use connection_watch::ConnectionWatch;
pub use reconnect_max_attempts_is_removed::ReconnectMaxAttemptsIsRemoved;
pub use sync_auth::SyncAuth;
pub use sync_client::SyncClient;
pub use sync_client_config::SyncClientConfig;
pub use transport_error::TransportError;
pub use unsigned::Unsigned;
pub use wait_cause::WaitCause;
pub use ws_remote_source::WsRemoteSource;
