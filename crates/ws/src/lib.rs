//! WebSocket server for the [Fundamental](https://docs.rs/fundamental) framework.
//!
//! Routes an inbound JSON envelope `{ "action": "…", "data": … }` to a typed handler, the shape
//! [`#[subscribe(socket = "…")]`](fundamental_macros::subscribe) targets, over `tokio-tungstenite` with no HTTP framework.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod config;
mod context;
mod error;
mod observability;
mod routing;
mod server;
mod session;
mod slots;
#[cfg(all(feature = "test-support", debug_assertions))]
pub mod test_support;

pub use config::WsConfig;
pub use context::WsContext;
pub use error::{WsError, WsResult};
pub use fundamental_core::error::PayloadRejection;
pub use fundamental_ws_wire::WsEnvelope;
pub use observability::{WsGauges, WsStats};
pub use routing::{Route, WsRouter};
pub use server::{WsListener, WsServer};
pub use session::{CloseReason, ConnectionId};
pub use slots::{Admission, AnonymousLimits};
