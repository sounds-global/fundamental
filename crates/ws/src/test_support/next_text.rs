//! [`next_text`] — the next reply a server sends, as JSON.

use futures::StreamExt;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

use crate::test_support::Socket;
use crate::test_support::until::PATIENCE;

/// Reads until the server's next text frame and returns it parsed as JSON.
///
/// # Panics
/// If no text frame arrives within twenty seconds, the connection closes first, or the frame is not JSON.
pub async fn next_text(socket: &mut Socket) -> serde_json::Value {
    loop {
        let frame = timeout(PATIENCE, socket.next())
            .await
            .expect("a frame in time")
            .expect("a frame before the end")
            .expect("an ok frame");
        match frame {
            Message::Text(text) => return serde_json::from_str(&text).expect("a JSON reply"),
            Message::Close(frame) => panic!("closed before the reply: {frame:?}"),
            _ => {}
        }
    }
}
