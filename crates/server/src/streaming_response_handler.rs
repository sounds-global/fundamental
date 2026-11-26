use std::future::Future;

use fundamental_core::AnyError;

use crate::StreamedResponse;
use crate::http_context::HttpContext;
use crate::http_handler::{ErasedHandler, HandlerFuture};

/// Adapts a streaming-response handler into an [`ErasedHandler`] whose body goes into the context's slot.
pub(crate) struct StreamingResponseHandler<H> {
    handler: H,
    /// What the route declares it answers with, when it says: a page's HTML.
    content_type: Option<&'static str>,
}

impl<H> StreamingResponseHandler<H> {
    pub(crate) fn new(handler: H) -> Self {
        Self {
            handler,
            content_type: None,
        }
    }

    /// A handler whose route declares `content_type`, such as a page's HTML.
    pub(crate) fn declaring(handler: H, content_type: &'static str) -> Self {
        Self {
            handler,
            content_type: Some(content_type),
        }
    }
}

impl<S, H, Fut> ErasedHandler<S> for StreamingResponseHandler<H>
where
    S: Send + 'static,
    H: Fn(HttpContext<S>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<StreamedResponse, AnyError>> + Send + 'static,
{
    fn content_type(&self) -> &'static str {
        self.content_type.unwrap_or("application/json")
    }

    fn call<'a>(&'a self, context: HttpContext<S>, _body: Vec<u8>) -> HandlerFuture<'a> {
        let slot = context.response_stream_slot();
        let fut = (self.handler)(context);
        Box::pin(async move {
            slot.put(fut.await?);
            Ok(Vec::new())
        })
    }
}
