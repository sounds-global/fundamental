//! The server: the [`WsServer`] acceptor and its [`WsListener`] guard.

mod abort_on_drop;
mod accept_loop;
mod drain;
mod drain_sessions;
mod drain_state;
mod ws_listener;
mod ws_server;

pub(crate) use drain::Drain;
pub use ws_listener::WsListener;
pub use ws_server::WsServer;
