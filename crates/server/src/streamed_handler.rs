use std::future::Future;
use std::marker::PhantomData;

use fundamental_core::AnyError;
use serde::Serialize;

use crate::RequestBodyStream;
use crate::body::encode_body;
use crate::http_context::HttpContext;
use crate::http_handler::{ErasedHandler, HandlerFuture};

/// Adapts a streamed-body handler into an [`ErasedHandler`], answering `O` as JSON.
pub(crate) struct StreamedHandler<S, O, H> {
    handler: H,
    _types: PhantomData<fn(S) -> O>,
}

impl<S, O, H> StreamedHandler<S, O, H> {
    pub(crate) fn new(handler: H) -> Self {
        Self {
            handler,
            _types: PhantomData,
        }
    }
}

impl<S, O, H, Fut> ErasedHandler<S> for StreamedHandler<S, O, H>
where
    S: Send + 'static,
    O: Serialize + Send + 'static,
    H: Fn(HttpContext<S>, RequestBodyStream) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
{
    fn call<'a>(&'a self, context: HttpContext<S>, body: Vec<u8>) -> HandlerFuture<'a> {
        let stream = context
            .take_body_stream()
            .unwrap_or_else(|| RequestBodyStream::buffered(body));
        let fut = (self.handler)(context, stream);
        Box::pin(async move {
            let output = fut.await?;
            encode_body(&output)
        })
    }
}
