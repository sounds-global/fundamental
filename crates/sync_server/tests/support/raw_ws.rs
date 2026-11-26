use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// A plain WebSocket client, so a suite can send what a real client never would.
pub type RawWs = WebSocketStream<MaybeTlsStream<TcpStream>>;
