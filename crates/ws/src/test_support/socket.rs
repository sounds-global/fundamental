//! [`Socket`] — the client end a suite drives.

use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// A client WebSocket connection to a server on loopback.
pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
