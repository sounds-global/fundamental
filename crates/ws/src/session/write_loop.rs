//! [`write_loop`]: delivers a connection's outbound frames, each within the send deadline.

use std::future::{pending, poll_fn};
use std::time::Duration;

use futures::{Sink, SinkExt};
use tokio::sync::mpsc::Receiver;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tracing::debug;

use crate::observability::millis;
use crate::session::CloseReason;

/// Sends every queued frame to `sink`, returning why the connection must end.
///
/// The sink is readied before a frame is taken, so no frame is held across an await and the goodbye
/// still flushes every frame when the session stops polling.
pub(crate) async fn write_loop<S>(
    sink: &mut S,
    inbox: &mut Receiver<Message>,
    send_timeout: Duration,
) -> CloseReason
where
    S: Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    loop {
        if let Some(reason) = within(send_timeout, poll_fn(|cx| sink.poll_ready_unpin(cx))).await {
            return reason;
        }
        let Some(message) = inbox.recv().await else {
            return pending().await;
        };
        if let Err(error) = sink.start_send_unpin(message) {
            debug!(%error, "error writing frame; closing connection");
            return CloseReason::TransportError;
        }
        if let Some(reason) = within(send_timeout, sink.flush()).await {
            return reason;
        }
    }
}

/// Runs one send step within the deadline, returning why the connection must end if it did not finish.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn within<E: std::fmt::Display>(
    send_timeout: Duration,
    step: impl Future<Output = Result<(), E>>,
) -> Option<CloseReason> {
    match timeout(send_timeout, step).await {
        Ok(Ok(())) => None,
        Ok(Err(error)) => {
            debug!(%error, "error writing frame; closing connection");
            Some(CloseReason::TransportError)
        }
        Err(_elapsed) => {
            debug!(
                send_timeout_ms = millis(send_timeout),
                "the peer did not take a frame within the send deadline"
            );
            Some(CloseReason::SlowPeer)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::sync::mpsc::channel;

    /// A sink that is never ready, as a peer that stopped reading looks.
    struct Stalled;

    impl Sink<Message> for Stalled {
        type Error = String;
        fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), String>> {
            Poll::Pending
        }
        fn start_send(self: Pin<&mut Self>, _: Message) -> Result<(), String> {
            Ok(())
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), String>> {
            Poll::Pending
        }
        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), String>> {
            Poll::Pending
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_stalled_peer_ends_the_connection_at_the_send_deadline() {
        let (tx, mut rx) = channel(1);
        tx.send(Message::text("hello")).await.expect("queued");
        let reason = write_loop(&mut Stalled, &mut rx, Duration::from_secs(10)).await;
        assert_eq!(reason, CloseReason::SlowPeer);
    }

    #[tokio::test(start_paused = true)]
    async fn a_writer_stopped_while_the_peer_is_slow_leaves_the_frame_in_the_queue() {
        let (tx, mut rx) = channel(1);
        tx.send(Message::text("reply")).await.expect("queued");
        let stopped = timeout(
            Duration::from_secs(1),
            write_loop(&mut Stalled, &mut rx, Duration::from_secs(10)),
        )
        .await;
        assert!(stopped.is_err(), "the writer was still waiting on the peer");
        assert_eq!(
            rx.try_recv().expect("the frame is still queued"),
            Message::text("reply")
        );
    }

    struct Reset;

    impl Sink<Message> for Reset {
        type Error = String;
        fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), String>> {
            Poll::Ready(Err("connection reset".to_string()))
        }
        fn start_send(self: Pin<&mut Self>, _: Message) -> Result<(), String> {
            Err("connection reset".to_string())
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), String>> {
            Poll::Ready(Ok(()))
        }
        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), String>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn a_failing_socket_ends_the_connection() {
        let (tx, mut rx) = channel(1);
        tx.send(Message::text("hello")).await.expect("queued");
        let reason = write_loop(&mut Reset, &mut rx, Duration::from_secs(10)).await;
        assert_eq!(reason, CloseReason::TransportError);
    }
}
