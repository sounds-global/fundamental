use std::future::Future;
use std::marker::PhantomData;

use fundamental_core::AnyError;
use serde::de::DeserializeOwned;

use crate::http_context::HttpContext;
use crate::http_handler::{ErasedHandler, HandlerFuture, InputSource, decode_input};

/// Adapts a typed handler returning `Vec<u8>` into an [`ErasedHandler`] answering those bytes under `content_type`.
pub(crate) struct BytesHandler<S, I, H> {
    handler: H,
    source: InputSource,
    content_type: &'static str,
    _types: PhantomData<fn(S, I)>,
}

impl<S, I, H> BytesHandler<S, I, H> {
    pub(crate) fn new(source: InputSource, content_type: &'static str, handler: H) -> Self {
        Self {
            handler,
            source,
            content_type,
            _types: PhantomData,
        }
    }
}

impl<S, I, H, Fut> ErasedHandler<S> for BytesHandler<S, I, H>
where
    S: Send + 'static,
    I: DeserializeOwned + Send + 'static,
    H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<u8>, AnyError>> + Send + 'static,
{
    fn call<'a>(&'a self, context: HttpContext<S>, body: Vec<u8>) -> HandlerFuture<'a> {
        match decode_input::<I>(
            self.source,
            context.query_raw(),
            &body,
            context.header("content-type"),
        ) {
            Ok(input) => Box::pin((self.handler)(context, input)),
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
    struct State;

    #[tokio::test]
    async fn returns_raw_bytes_under_its_content_type() {
        let handler = BytesHandler::new(
            InputSource::Query,
            "application/octet-stream",
            |_ctx: HttpContext<State>, _input: ()| async move { Ok(vec![0u8, 1, 2, 255]) },
        );
        let ctx = HttpContext::new(State, "req-1");
        let body = handler.call(ctx, Vec::new()).await.expect("renders");
        assert_eq!(body, vec![0u8, 1, 2, 255]);
        assert_eq!(handler.content_type(), "application/octet-stream");
    }

    #[tokio::test]
    async fn decodes_a_form_body_or_a_json_one_by_its_declared_type() {
        #[derive(serde::Deserialize)]
        struct Wanted {
            name: String,
        }
        let handler = BytesHandler::new(
            InputSource::BodyOrForm,
            "text/csv",
            |_ctx: HttpContext<State>, input: Wanted| async move { Ok(input.name.into_bytes()) },
        );
        let form = HttpContext::new(State, "req-1").with_headers(
            [(
                "content-type".to_owned(),
                "application/x-www-form-urlencoded; charset=UTF-8".to_owned(),
            )]
            .into_iter()
            .collect(),
        );
        let body = handler
            .call(form, b"name=ada%20lovelace".to_vec())
            .await
            .expect("a form post decodes");
        assert_eq!(body, b"ada lovelace");
        let json = HttpContext::new(State, "req-2");
        let body = handler
            .call(json, br#"{"name":"grace"}"#.to_vec())
            .await
            .expect("a JSON post decodes");
        assert_eq!(body, b"grace");
    }

    #[tokio::test]
    async fn propagates_a_handler_error() {
        let handler = BytesHandler::new(
            InputSource::Query,
            "application/octet-stream",
            |_ctx: HttpContext<State>, _input: ()| async move {
                Err(AnyError::new(crate::ServerError::serialize(
                    "boom".to_owned(),
                )))
            },
        );
        let ctx = HttpContext::new(State, "req-1");
        assert!(handler.call(ctx, Vec::new()).await.is_err());
    }
}
