//! [`connect_to`] — a client connection to a server known only by its address.

use std::net::SocketAddr;

use tokio_tungstenite::connect_async;

use crate::test_support::Socket;

/// Connects a client to the server at `addr`.
///
/// # Panics
/// If the connection or its upgrade fails.
pub async fn connect_to(addr: SocketAddr) -> Socket {
    connect_async(format!("ws://{addr}"))
        .await
        .expect("the client connects")
        .0
}
