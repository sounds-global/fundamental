use std::future::Future;
use std::marker::PhantomData;

use fundamental_core::AnyError;
use serde::de::DeserializeOwned;

use crate::http_context::HttpContext;
use crate::http_handler::{ErasedHandler, HandlerFuture, InputSource, decode_input};

/// Adapts a typed handler returning `String` into an [`ErasedHandler`] answering it raw under `content_type`.
pub(crate) struct RawHandler<S, I, H> {
    handler: H,
    source: InputSource,
    content_type: &'static str,
    _types: PhantomData<fn(S, I)>,
}

impl<S, I, H> RawHandler<S, I, H> {
    pub(crate) fn new(source: InputSource, content_type: &'static str, handler: H) -> Self {
        Self {
            handler,
            source,
            content_type,
            _types: PhantomData,
        }
    }
}

impl<S, I, H, Fut> ErasedHandler<S> for RawHandler<S, I, H>
where
    S: Send + 'static,
    I: DeserializeOwned + Send + 'static,
    H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String, AnyError>> + Send + 'static,
{
    fn call<'a>(&'a self, context: HttpContext<S>, body: Vec<u8>) -> HandlerFuture<'a> {
        match decode_input::<I>(
            self.source,
            context.query_raw(),
            &body,
            context.header("content-type"),
        ) {
            Ok(input) => {
                let fut = (self.handler)(context, input);
                Box::pin(async move { fut.await.map(String::into_bytes) })
            }
            Err(err) => Box::pin(async move { Err(err) }),
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
    struct State {
        name: &'static str,
    }

    #[tokio::test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn renders_a_body_under_its_content_type() {
        let handler = RawHandler::new(
            InputSource::Body,
            "text/html; charset=utf-8",
            |ctx: HttpContext<State>, _input: ()| {
                let name = ctx.state().name;
                async move { Ok(format!("<h1>{name}</h1>")) }
            },
        );
        let ctx = HttpContext::new(State { name: "Ada" }, "req-1");
        let body = handler.call(ctx, Vec::new()).await.expect("renders");
        assert_eq!(body, b"<h1>Ada</h1>");
        assert_eq!(handler.content_type(), "text/html; charset=utf-8");
    }

    #[tokio::test]
    async fn emits_a_query_bound_input_as_a_signed_body() {
        #[derive(serde::Deserialize)]
        struct Query {
            sub: String,
        }
        let handler = RawHandler::new(
            InputSource::Query,
            "application/jwt",
            |_ctx: HttpContext<State>, input: Query| async move { Ok(format!("jwt-for-{}", input.sub)) },
        );
        let ctx = HttpContext::new(State { name: "x" }, "req-1").with_query("sub=alice");
        let body = handler.call(ctx, Vec::new()).await.expect("renders");
        assert_eq!(body, b"jwt-for-alice");
        assert_eq!(handler.content_type(), "application/jwt");
    }

    #[tokio::test]
    async fn propagates_a_render_error() {
        let handler = RawHandler::new(
            InputSource::Body,
            "application/jwt",
            |_ctx: HttpContext<State>, _input: ()| async move {
                Err(AnyError::new(crate::ServerError::serialize(
                    "boom".to_owned(),
                )))
            },
        );
        let ctx = HttpContext::new(State { name: "x" }, "req-1");
        let err = handler.call(ctx, Vec::new()).await.expect_err("fails");
        assert_eq!(err.get().error_code(), "API_008");
    }
}
