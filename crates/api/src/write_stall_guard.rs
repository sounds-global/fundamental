use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{Sleep, sleep};
use tracing::info;

/// A stream whose writes fail once they have made no progress for `timeout`.
pub(crate) struct WriteStallGuard<S> {
    inner: S,
    timeout: Duration,
    stalled: Option<Pin<Box<Sleep>>>,
}

impl<S> WriteStallGuard<S> {
    pub(crate) fn new(inner: S, timeout: Duration) -> Self {
        Self {
            inner,
            timeout,
            stalled: None,
        }
    }

    /// Passes a write's outcome through, failing it once the stall timer runs out.
    fn watch<T>(
        &mut self,
        cx: &mut Context<'_>,
        outcome: Poll<io::Result<T>>,
    ) -> Poll<io::Result<T>> {
        match outcome {
            Poll::Ready(outcome) => {
                self.stalled = None;
                Poll::Ready(outcome)
            }
            Poll::Pending => {
                let timeout = self.timeout;
                let stalled = self.stalled.get_or_insert_with(|| Box::pin(sleep(timeout)));
                match stalled.as_mut().poll(cx) {
                    Poll::Ready(()) => {
                        info!(
                            stalled_secs = timeout.as_secs(),
                            "a connection's writes made no progress; closing it"
                        );
                        Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "the peer stopped reading",
                        )))
                    }
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for WriteStallGuard<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for WriteStallGuard<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let outcome = Pin::new(&mut self.inner).poll_write(cx, buf);
        self.watch(cx, outcome)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let outcome = Pin::new(&mut self.inner).poll_write_vectored(cx, bufs);
        self.watch(cx, outcome)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let outcome = Pin::new(&mut self.inner).poll_flush(cx);
        self.watch(cx, outcome)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncWriteExt, duplex};

    use super::WriteStallGuard;

    #[tokio::test(start_paused = true)]
    async fn a_write_the_peer_never_reads_fails_after_the_timeout() {
        // A pipe of 64 bytes whose far end is never read: the second write cannot complete.
        let (near, _far) = duplex(64);
        let mut guarded = WriteStallGuard::new(near, Duration::from_secs(30));
        guarded.write_all(&[1; 64]).await.expect("fits the pipe");
        let stalled = guarded.write_all(&[2; 64]).await.expect_err("never read");
        assert_eq!(stalled.kind(), std::io::ErrorKind::TimedOut);
    }

    #[tokio::test(start_paused = true)]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn a_slow_reader_that_keeps_reading_is_never_cut() {
        let (near, mut far) = duplex(64);
        let mut guarded = WriteStallGuard::new(near, Duration::from_secs(30));
        let reader = tokio::spawn(async move {
            let mut buffer = [0_u8; 64];
            let mut total = 0;
            while total < 64 * 10 {
                tokio::time::sleep(Duration::from_secs(20)).await;
                total += tokio::io::AsyncReadExt::read(&mut far, &mut buffer)
                    .await
                    .expect("read");
            }
            total
        });
        for _ in 0..10 {
            guarded
                .write_all(&[3; 64])
                .await
                .expect("each write moves within the timeout");
        }
        assert_eq!(reader.await.expect("joined"), 640);
    }
}
