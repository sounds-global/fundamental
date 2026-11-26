use std::future::Future;
use std::marker::PhantomData;

use fundamental_core::AnyError;

use crate::http_context::HttpContext;
use crate::http_handler::{ErasedHandler, HandlerFuture};

/// Adapts a `Vec<u8>`-in, `Vec<u8>`-out handler into an [`ErasedHandler`], decoding nothing.
pub(crate) struct BytesIoHandler<S, H> {
    handler: H,
    content_type: &'static str,
    _state: PhantomData<fn(S)>,
}

impl<S, H> BytesIoHandler<S, H> {
    pub(crate) fn new(content_type: &'static str, handler: H) -> Self {
        Self {
            handler,
            content_type,
            _state: PhantomData,
        }
    }
}

impl<S, H, Fut> ErasedHandler<S> for BytesIoHandler<S, H>
where
    S: Send + 'static,
    H: Fn(HttpContext<S>, Vec<u8>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<u8>, AnyError>> + Send + 'static,
{
    fn call<'a>(&'a self, context: HttpContext<S>, body: Vec<u8>) -> HandlerFuture<'a> {
        let fut = (self.handler)(context, body);
        Box::pin(fut)
    }

    fn content_type(&self) -> &'static str {
        self.content_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct State;

    /// The defining property: arbitrary bytes in, arbitrary bytes out, unchanged.
    #[tokio::test]
    async fn passes_binary_bodies_through_untouched() {
        let handler = BytesIoHandler::new(
            "message/ohttp-res",
            |_ctx: HttpContext<State>, body: Vec<u8>| async move {
                let mut out = b"echo:".to_vec();
                out.extend_from_slice(&body);
                Ok(out)
            },
        );
        let ctx = HttpContext::new(State, "req-1");
        let out = handler
            .call(ctx, vec![0xff, 0xfe, 0x00, 0x80])
            .await
            .expect("runs");
        assert_eq!(out, b"echo:\xff\xfe\x00\x80");
        assert_eq!(handler.content_type(), "message/ohttp-res");
    }

    // HPKE ciphertext is essentially never valid UTF-8.
    #[tokio::test]
    async fn a_non_utf8_body_is_not_an_error() {
        let handler = BytesIoHandler::new(
            "application/octet-stream",
            |_ctx: HttpContext<State>, body: Vec<u8>| async move { Ok(body) },
        );
        let ctx = HttpContext::new(State, "req-1");
        let body = vec![0xff, 0xfe, 0xfd];
        let out = handler
            .call(ctx, body.clone())
            .await
            .expect("accepts bytes");
        assert_eq!(out, body);
    }

    /// An empty body is a body, not a missing one.
    #[tokio::test]
    async fn an_empty_body_is_passed_through() {
        let handler = BytesIoHandler::new(
            "application/octet-stream",
            |_ctx: HttpContext<State>, body: Vec<u8>| async move {
                assert!(body.is_empty());
                Ok(Vec::new())
            },
        );
        let ctx = HttpContext::new(State, "req-1");
        assert!(
            handler
                .call(ctx, Vec::new())
                .await
                .expect("runs")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_handler_error_propagates() {
        let handler = BytesIoHandler::new(
            "application/octet-stream",
            |_ctx: HttpContext<State>, _body: Vec<u8>| async move {
                Err(AnyError::new(crate::ServerError::deserialize(
                    "the protocol seam refused it".to_string(),
                )))
            },
        );
        let ctx = HttpContext::new(State, "req-1");
        let err = handler.call(ctx, vec![1]).await.expect_err("refuses");
        assert_eq!(err.get().http_status_code(), 400);
    }
}
