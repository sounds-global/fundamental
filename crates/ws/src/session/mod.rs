//! Per-connection state: the connection id, why a connection ended, and its session.

mod close_reason;
mod connection_id;
pub(crate) mod read_loop;
pub(crate) mod write_loop;
pub(crate) mod ws_session;

pub use close_reason::CloseReason;
pub use connection_id::ConnectionId;
pub(crate) use ws_session::CLOSE_GRACE;
