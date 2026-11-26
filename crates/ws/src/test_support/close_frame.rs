//! [`close_frame`] — the code and reason a server closed a connection with.

use futures::StreamExt;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

use crate::test_support::Socket;
use crate::test_support::until::PATIENCE;

/// Reads until the server's close frame, returning its code and reason.
///
/// # Panics
/// If no close frame arrives within ten seconds, or the connection ends without one.
pub async fn close_frame(socket: &mut Socket) -> (u16, String) {
    loop {
        let frame = timeout(PATIENCE, socket.next())
            .await
            .expect("closed in time")
            .expect("a close frame before the end")
            .expect("an ok frame");
        if let Message::Close(Some(frame)) = frame {
            return (u16::from(frame.code), frame.reason.to_string());
        }
    }
}
