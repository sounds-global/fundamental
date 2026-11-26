//! [`connect`] — a client connection to a served listener.

use crate::WsListener;
use crate::test_support::{Socket, connect_to};

/// Connects a client to `listener`.
///
/// # Panics
/// If the connection or its upgrade fails.
pub async fn connect(listener: &WsListener) -> Socket {
    connect_to(listener.local_addr()).await
}
