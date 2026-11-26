use crate::from_ref::FromRef;
use crate::http_context::HttpContext;
use crate::http_handler::{ErasedHandler, HandlerFuture};
use crate::middleware::ErasedMiddleware;
use crate::next::Next;

/// A route written against `Sub`, its chain and handler run as one handler on a router over `Outer`.
pub(crate) struct ProjectedHandler<Sub> {
    /// The route's own chain, in order.
    middleware: Vec<Box<dyn ErasedMiddleware<Sub>>>,
    /// What the chain ends at.
    handler: Box<dyn ErasedHandler<Sub>>,
}

impl<Sub> ProjectedHandler<Sub> {
    pub(crate) const fn new(
        middleware: Vec<Box<dyn ErasedMiddleware<Sub>>>,
        handler: Box<dyn ErasedHandler<Sub>>,
    ) -> Self {
        Self {
            middleware,
            handler,
        }
    }
}

impl<Sub, Outer> ErasedHandler<Outer> for ProjectedHandler<Sub>
where
    Sub: FromRef<Outer> + Send + Sync + 'static,
    Outer: Send + Sync + 'static,
{
    fn call<'a>(&'a self, context: HttpContext<Outer>, body: Vec<u8>) -> HandlerFuture<'a> {
        // The router's chain has already run, outside this handler.
        let next = Next::new(&[], &self.middleware, self.handler.as_ref());
        next.run(context.project::<Sub>(), body)
    }

    /// The inner handler's content type, so a projected page still answers `text/html`.
    fn content_type(&self) -> &'static str {
        self.handler.content_type()
    }
}
