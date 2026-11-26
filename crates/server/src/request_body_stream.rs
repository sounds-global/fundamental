use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;

use crate::ServerError;

/// The body of a request to a [streamed route](crate::Route::streamed), a chunk at a time, bounded by the backend.
pub struct RequestBodyStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, ServerError>> + Send>>,
}

impl RequestBodyStream {
    /// Wraps a backend's bounded stream.
    pub fn new(stream: impl Stream<Item = Result<Bytes, ServerError>> + Send + 'static) -> Self {
        Self {
            inner: Box::pin(stream),
        }
    }

    /// A body already read whole, yielded as one chunk.
    #[must_use]
    pub fn buffered(body: Vec<u8>) -> Self {
        Self::new(Buffered(Some(Bytes::from(body))))
    }

    /// The next chunk, or `None` once the body has ended.
    pub async fn next_chunk(&mut self) -> Option<Result<Bytes, ServerError>> {
        std::future::poll_fn(|cx| self.inner.as_mut().poll_next(cx)).await
    }
}

impl Stream for RequestBodyStream {
    type Item = Result<Bytes, ServerError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

impl fmt::Debug for RequestBodyStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RequestBodyStream")
    }
}

struct Buffered(Option<Bytes>);

impl Stream for Buffered {
    type Item = Result<Bytes, ServerError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().filter(|body| !body.is_empty()).map(Ok))
    }
}

#[cfg(test)]
mod tests {
    use super::RequestBodyStream;

    #[tokio::test]
    async fn a_buffered_body_is_one_chunk_and_an_empty_one_none() {
        let mut body = RequestBodyStream::buffered(b"abc".to_vec());
        assert_eq!(
            body.next_chunk().await.and_then(Result::ok).as_deref(),
            Some(&b"abc"[..])
        );
        assert!(body.next_chunk().await.is_none());
        assert!(
            RequestBodyStream::buffered(Vec::new())
                .next_chunk()
                .await
                .is_none()
        );
    }
}
