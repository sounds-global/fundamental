use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{BodyDataStream, Bytes};
use fundamental_server::{ServerError, StreamBounds};
use futures_core::Stream;
use tokio::time::{Instant, Sleep, sleep};
use tracing::debug;

/// A streamed route's body, ended with the [`ServerError`] naming the first bound it crosses.
pub(crate) struct GuardedBody {
    inner: BodyDataStream,
    max_bytes: u64,
    read: u64,
    idle: Duration,
    idle_timer: Pin<Box<Sleep>>,
    started: Instant,
    grace: Duration,
    min_bytes_per_sec: u64,
    done: bool,
}

impl GuardedBody {
    pub(crate) fn new(inner: BodyDataStream, bounds: &StreamBounds) -> Self {
        Self {
            inner,
            max_bytes: bounds.max_bytes(),
            read: 0,
            idle: bounds.idle(),
            idle_timer: Box::pin(sleep(bounds.idle())),
            started: Instant::now(),
            grace: bounds.grace(),
            min_bytes_per_sec: bounds.min_bytes_per_sec(),
            done: false,
        }
    }

    fn end(&mut self, error: ServerError) -> Poll<Option<Result<Bytes, ServerError>>> {
        self.done = true;
        debug!(
            error = fundamental_core::Error::error_code(&error),
            read = self.read,
            "a streamed body was cut off"
        );
        Poll::Ready(Some(Err(error)))
    }

    fn below_minimum_rate(&self) -> bool {
        let elapsed = self.started.elapsed();
        elapsed > self.grace
            && u128::from(self.read) * 1_000
                < u128::from(self.min_bytes_per_sec) * elapsed.as_millis()
    }
}

impl Stream for GuardedBody {
    type Item = Result<Bytes, ServerError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.read += chunk.len() as u64;
                if self.read > self.max_bytes {
                    let max = self.max_bytes;
                    return self.end(ServerError::PayloadTooLarge { max });
                }
                if self.below_minimum_rate() {
                    return self.end(ServerError::BodyTooSlow {
                        reason: "fell below its minimum rate",
                    });
                }
                let deadline = Instant::now() + self.idle;
                self.idle_timer.as_mut().reset(deadline);
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(_))) => self.end(ServerError::BodyInterrupted),
            Poll::Ready(None) => {
                self.done = true;
                Poll::Ready(None)
            }
            Poll::Pending => match self.idle_timer.as_mut().poll(cx) {
                Poll::Ready(()) => self.end(ServerError::BodyTooSlow {
                    reason: "went idle",
                }),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GuardedBody;
    use axum::body::{Body, Bytes};
    use fundamental_server::{ServerError, StreamBounds};
    use std::convert::Infallible;
    use std::future::poll_fn;
    use std::pin::Pin;
    use std::time::Duration;

    async fn next(body: &mut GuardedBody) -> Option<Result<Bytes, ServerError>> {
        poll_fn(|cx| futures_core::Stream::poll_next(Pin::new(&mut *body), cx)).await
    }

    fn chunks(parts: Vec<&'static [u8]>) -> Body {
        let items: Vec<Result<Bytes, Infallible>> = parts
            .into_iter()
            .map(|part| Ok(Bytes::from_static(part)))
            .collect();
        Body::from_stream(Iter(items.into_iter()))
    }

    struct Iter(std::vec::IntoIter<Result<Bytes, Infallible>>);
    impl futures_core::Stream for Iter {
        type Item = Result<Bytes, Infallible>;
        fn poll_next(
            mut self: Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::task::Poll::Ready(self.0.next())
        }
    }

    #[tokio::test]
    async fn a_body_over_the_route_bound_is_api_020() {
        let bounds = StreamBounds::new(5);
        let mut body = GuardedBody::new(chunks(vec![b"abc", b"def"]).into_data_stream(), &bounds);
        assert!(matches!(next(&mut body).await, Some(Ok(_))));
        assert!(matches!(
            next(&mut body).await,
            Some(Err(ServerError::PayloadTooLarge { max: 5 }))
        ));
        assert!(
            next(&mut body).await.is_none(),
            "nothing is read past the bound"
        );
    }

    #[tokio::test]
    async fn a_body_that_breaks_off_is_api_026() {
        struct BrokenOff(u8);
        impl futures_core::Stream for BrokenOff {
            type Item = Result<Bytes, std::io::Error>;
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn poll_next(
                mut self: Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<Self::Item>> {
                self.0 += 1;
                std::task::Poll::Ready(Some(if self.0 == 1 {
                    Ok(Bytes::from_static(b"the first part"))
                } else {
                    Err(std::io::Error::from(std::io::ErrorKind::ConnectionReset))
                }))
            }
        }
        let bounds = StreamBounds::new(1_000);
        let mut body =
            GuardedBody::new(Body::from_stream(BrokenOff(0)).into_data_stream(), &bounds);
        assert!(matches!(next(&mut body).await, Some(Ok(_))));
        let broken = next(&mut body).await;
        assert!(
            matches!(broken, Some(Err(ServerError::BodyInterrupted))),
            "{broken:?}"
        );
        assert_eq!(
            fundamental_core::Error::error_code(&ServerError::BodyInterrupted),
            "API_026"
        );
        assert!(next(&mut body).await.is_none(), "nothing is read after it");
    }

    #[tokio::test(start_paused = true)]
    async fn an_idle_body_is_cut() {
        let bounds = StreamBounds::new(100).with_idle(Duration::from_secs(2));
        let (_sender, never) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(1);
        let stalled = Body::from_stream(Stalled(never));
        let mut body = GuardedBody::new(stalled.into_data_stream(), &bounds);
        assert!(matches!(
            next(&mut body).await,
            Some(Err(ServerError::BodyTooSlow {
                reason: "went idle"
            }))
        ));
    }

    #[tokio::test(start_paused = true)]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn a_trickled_body_is_cut() {
        let bounds = StreamBounds::new(1_000_000)
            .with_idle(Duration::from_secs(60))
            .with_min_rate(1_000, Duration::from_secs(5));
        let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(4);
        let mut body = GuardedBody::new(
            Body::from_stream(Stalled(receiver)).into_data_stream(),
            &bounds,
        );
        tokio::spawn(async move {
            for _ in 0..20 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if sender.send(Ok(Bytes::from_static(b"x"))).await.is_err() {
                    return;
                }
            }
        });
        let mut outcome = None;
        while let Some(item) = next(&mut body).await {
            if let Err(error) = item {
                outcome = Some(error);
            }
        }
        assert!(
            matches!(
                outcome,
                Some(ServerError::BodyTooSlow {
                    reason: "fell below its minimum rate"
                })
            ),
            "{outcome:?}"
        );
    }

    struct Stalled(tokio::sync::mpsc::Receiver<Result<Bytes, Infallible>>);
    impl futures_core::Stream for Stalled {
        type Item = Result<Bytes, Infallible>;
        fn poll_next(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            self.0.poll_recv(cx)
        }
    }
}
