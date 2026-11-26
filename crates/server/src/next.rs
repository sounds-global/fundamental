use crate::http_context::HttpContext;
use crate::http_handler::ErasedHandler;
use crate::middleware::{ErasedMiddleware, MiddlewareFuture};
use crate::segment_stage::SegmentStage;

/// The rest of a request's middleware chain, ending at the route's handler.
///
/// Two borrowed slices, the router's and the route's, so composing them allocates nothing.
pub struct Next<'a, S> {
    outer: &'a [Box<dyn ErasedMiddleware<S>>],
    inner: &'a [Box<dyn ErasedMiddleware<S>>],
    handler: &'a dyn ErasedHandler<S>,
    /// The route's declared segments, resolved between `outer` and `inner`.
    stage: Option<SegmentStage<'a>>,
}

impl<'a, S> Next<'a, S>
where
    S: Send + Sync + 'static,
{
    /// Creates the continuation: `outer` (router-level) runs before `inner` (route-level).
    pub(crate) fn new(
        outer: &'a [Box<dyn ErasedMiddleware<S>>],
        inner: &'a [Box<dyn ErasedMiddleware<S>>],
        handler: &'a dyn ErasedHandler<S>,
    ) -> Self {
        Self {
            outer,
            inner,
            handler,
            stage: None,
        }
    }

    /// As [`new`](Self::new), resolving `stage` between the two chains.
    pub(crate) fn staged(
        outer: &'a [Box<dyn ErasedMiddleware<S>>],
        inner: &'a [Box<dyn ErasedMiddleware<S>>],
        handler: &'a dyn ErasedHandler<S>,
        stage: Option<SegmentStage<'a>>,
    ) -> Self {
        Self {
            outer,
            inner,
            handler,
            stage,
        }
    }

    /// Runs the rest of the chain: the next middleware if one remains, otherwise the handler.
    pub fn run(self, ctx: HttpContext<S>, body: Vec<u8>) -> MiddlewareFuture<'a> {
        if let Some((head, tail)) = self.outer.split_first() {
            return head.handle(
                ctx,
                body,
                Next::staged(tail, self.inner, self.handler, self.stage),
            );
        }
        if let Some(stage) = self.stage {
            return Box::pin(stage.run(ctx, body, Next::new(self.outer, self.inner, self.handler)));
        }
        match self.inner.split_first() {
            Some((head, tail)) => head.handle(ctx, body, Next::new(self.outer, tail, self.handler)),
            None => self.handler.call(ctx, body),
        }
    }
}
