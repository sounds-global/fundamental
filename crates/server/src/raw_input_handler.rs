use std::future::Future;
use std::marker::PhantomData;

use fundamental_core::AnyError;

use crate::ServerError;
use crate::http_context::HttpContext;
use crate::http_handler::{ErasedHandler, HandlerFuture};

/// Adapts a `String`-body handler into an [`ErasedHandler`] answering its `String` under `content_type`.
pub(crate) struct RawInputHandler<S, H> {
    handler: H,
    content_type: &'static str,
    _state: PhantomData<fn(S)>,
}

impl<S, H> RawInputHandler<S, H> {
    pub(crate) fn new(content_type: &'static str, handler: H) -> Self {
        Self {
            handler,
            content_type,
            _state: PhantomData,
        }
    }
}

impl<S, H, Fut> ErasedHandler<S> for RawInputHandler<S, H>
where
    S: Send + 'static,
    H: Fn(HttpContext<S>, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, AnyError>> + Send + 'static,
{
    fn call<'a>(&'a self, context: HttpContext<S>, body: Vec<u8>) -> HandlerFuture<'a> {
        // A non-UTF-8 body is a bad request, never silently lossy.
        match String::from_utf8(body) {
            Ok(input) => {
                let fut = (self.handler)(context, input);
                Box::pin(async move { fut.await.map(String::into_bytes) })
            }
            Err(error) => Box::pin(async move {
                Err(AnyError::new(ServerError::deserialize(format!(
                    "request body is not valid UTF-8: {error}"
                ))))
            }),
        }
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

    #[tokio::test]
    async fn passes_the_raw_body_to_the_handler() {
        let handler = RawInputHandler::new(
            "text/plain; charset=utf-8",
            |_ctx: HttpContext<State>, body: String| async move { Ok(format!("echo:{body}")) },
        );
        let ctx = HttpContext::new(State, "req-1");
        let out = handler
            .call(ctx, b"old 3\n\nacme.audit.test\n3\nAAAA\n".to_vec())
            .await
            .expect("runs");
        assert_eq!(out, b"echo:old 3\n\nacme.audit.test\n3\nAAAA\n");
        assert_eq!(handler.content_type(), "text/plain; charset=utf-8");
    }

    #[tokio::test]
    async fn a_non_utf8_body_is_a_bad_request() {
        let handler = RawInputHandler::new(
            "text/plain; charset=utf-8",
            |_ctx: HttpContext<State>, body: String| async move { Ok(body) },
        );
        let ctx = HttpContext::new(State, "req-1");
        let err = handler
            .call(ctx, vec![0xff, 0xfe])
            .await
            .expect_err("rejects non-utf8");
        assert_eq!(err.get().error_code(), "API_007");
        assert_eq!(err.get().http_status_code(), 400);
    }
}
