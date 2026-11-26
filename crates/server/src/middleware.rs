use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;

use crate::http_context::HttpContext;
use crate::next::Next;

/// What a middleware and a handler both return: the serialized response body, or an [`AnyError`].
pub type MiddlewareResult = Result<Vec<u8>, AnyError>;

/// A layer wrapping a route's handler; write one with [`#[middleware]`](macro@fundamental_macros::middleware).
pub trait Middleware<S>: Send + Sync + 'static {
    /// Whether this layer can only ever be satisfied by a verified principal (`#[middleware(needs_principal)]`).
    ///
    /// Lets the API server refuse a gated route mounted without a verifier.
    const NEEDS_PRINCIPAL: bool = false;

    /// Whether this layer decides which object the caller may act on (`#[middleware(grants_authorization)]`).
    const GRANTS_AUTHORIZATION: bool = false;

    /// Whether this layer takes the object it authorizes from the request path.
    const AUTHORIZES_FROM_PATH: bool = false;

    /// Whether this layer reads the request body (`#[middleware(reads_body)]`); a streamed route hands it none.
    const READS_BODY: bool = false;

    /// Handles the request, calling [`next`](Next::run) to continue the chain or returning early.
    fn handle(
        &self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'_, S>,
    ) -> impl Future<Output = MiddlewareResult> + Send;
}

/// A middleware future borrowing the chain (and the middleware) for `'a`.
pub(crate) type MiddlewareFuture<'a> = Pin<Box<dyn Future<Output = MiddlewareResult> + Send + 'a>>;

/// An object-safe view of a [`Middleware`], its returned future boxed.
pub(crate) trait ErasedMiddleware<S>: Send + Sync {
    fn handle<'a>(
        &'a self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'a, S>,
    ) -> MiddlewareFuture<'a>;
}

impl<S, M> ErasedMiddleware<S> for M
where
    M: Middleware<S>,
    S: Send + Sync + 'static,
{
    fn handle<'a>(
        &'a self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'a, S>,
    ) -> MiddlewareFuture<'a> {
        Box::pin(Middleware::handle(self, ctx, body, next))
    }
}
