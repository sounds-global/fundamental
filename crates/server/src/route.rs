use std::future::Future;
use std::sync::Arc;

use fundamental_core::AnyError;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::action_call::ActionCall;
use crate::bytes_handler::BytesHandler;
use crate::bytes_io_handler::BytesIoHandler;
use crate::from_ref::FromRef;
use crate::http_context::HttpContext;
use crate::http_handler::{ErasedHandler, InputSource, TypedHandler};
use crate::middleware::{ErasedMiddleware, Middleware, MiddlewareFuture, MiddlewareResult};
use crate::next::Next;
use crate::projected_handler::ProjectedHandler;
use crate::raw_handler::RawHandler;
use crate::raw_input_handler::RawInputHandler;
use crate::route_pattern::RoutePattern;
use crate::segment_resolver::ErasedSegmentResolver;
use crate::segment_stage::SegmentStage;
use crate::streamed_handler::StreamedHandler;
use crate::streaming_response_handler::StreamingResponseHandler;
use crate::{
    BodyPolicy, Method, PathParams, RequestBodyStream, ServerError, StreamBounds, StreamedResponse,
};

/// A single route: a method, a path template, the handler that serves it, and its middleware chain.
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
pub struct Route<S> {
    method: Method,
    pattern: RoutePattern,
    middleware: Vec<Box<dyn ErasedMiddleware<S>>>,
    handler: Box<dyn ErasedHandler<S>>,
    /// Whether any middleware declared [`Middleware::NEEDS_PRINCIPAL`], recorded before its type is erased.
    needs_principal: bool,
    /// Whether any middleware declared [`Middleware::GRANTS_AUTHORIZATION`], recorded likewise.
    grants_authorization: bool,
    /// Whether this route declared that its act names no single object.
    no_single_object: bool,
    /// Whether the chain's authorizing layer takes its object from the path.
    authorizes_from_path: bool,
    /// How the backend reads this route's body.
    body_policy: BodyPolicy,
    /// Whether any middleware in the chain declared [`Middleware::READS_BODY`].
    reads_body: bool,
    /// Whether declared segments are left unresolved: a retired template, which must cost no lookup.
    skips_segments: bool,
    /// The origin set this route answers across origins, reading no credential.
    cross_origin: Option<&'static str>,
}

impl<S> Route<S>
where
    S: Send + Sync + 'static,
{
    /// Binds `method` and `path` (a template such as `"/users/{id}"`) to `handler`.
    pub fn new<I, O, H, Fut>(method: Method, path: impl Into<String>, handler: H) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
    {
        Self::bind(method, &path.into(), Box::new(TypedHandler::new(handler)))
    }

    /// The one place a `Route` is constructed, so no constructor can forget a declaration field.
    fn bind(method: Method, path: &str, handler: Box<dyn ErasedHandler<S>>) -> Self {
        Self {
            method,
            pattern: RoutePattern::parse(path),
            middleware: Vec::new(),
            handler,
            needs_principal: false,
            grants_authorization: false,
            no_single_object: false,
            authorizes_from_path: false,
            body_policy: BodyPolicy::Buffered,
            reads_body: false,
            skips_segments: false,
            cross_origin: None,
        }
    }

    /// Binds a route whose handler reads the request body as it arrives, under `bounds`, and answers JSON.
    ///
    /// Nothing of the body is read until the middleware chain has run; the chain is handed an empty body.
    pub fn streamed<O, H, Fut>(
        method: Method,
        path: impl Into<String>,
        bounds: StreamBounds,
        handler: H,
    ) -> Self
    where
        O: Serialize + Send + 'static,
        H: Fn(HttpContext<S>, RequestBodyStream) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
    {
        let mut route = Self::bind(
            method,
            &path.into(),
            Box::new(StreamedHandler::new(handler)),
        );
        route.body_policy = BodyPolicy::Streamed(bounds);
        route
    }

    /// Binds a route whose handler answers with a body written as it is produced.
    pub fn streaming_response<H, Fut>(method: Method, path: impl Into<String>, handler: H) -> Self
    where
        H: Fn(HttpContext<S>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<StreamedResponse, AnyError>> + Send + 'static,
    {
        Self::bind(
            method,
            &path.into(),
            Box::new(StreamingResponseHandler::new(handler)),
        )
    }

    /// Binds a route that answers with a page's HTML written as it is rendered, found by [`Router::serves_page`](crate::Router::serves_page).
    pub fn streaming_page<H, Fut>(method: Method, path: impl Into<String>, handler: H) -> Self
    where
        H: Fn(HttpContext<S>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<StreamedResponse, AnyError>> + Send + 'static,
    {
        Self::bind(
            method,
            &path.into(),
            Box::new(StreamingResponseHandler::declaring(
                handler,
                "text/html; charset=utf-8",
            )),
        )
    }

    /// Binds a `GET` route — see [`Route::new`].
    pub fn get<I, O, H, Fut>(path: impl Into<String>, handler: H) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
    {
        Self::new(Method::Get, path, handler)
    }

    /// Binds a `POST` route — see [`Route::new`].
    pub fn post<I, O, H, Fut>(path: impl Into<String>, handler: H) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
    {
        Self::new(Method::Post, path, handler)
    }

    /// Binds a `PUT` route — see [`Route::new`].
    pub fn put<I, O, H, Fut>(path: impl Into<String>, handler: H) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
    {
        Self::new(Method::Put, path, handler)
    }

    /// Binds a `PATCH` route — see [`Route::new`].
    pub fn patch<I, O, H, Fut>(path: impl Into<String>, handler: H) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
    {
        Self::new(Method::Patch, path, handler)
    }

    /// Binds a `DELETE` route — see [`Route::new`].
    pub fn delete<I, O, H, Fut>(path: impl Into<String>, handler: H) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
    {
        Self::new(Method::Delete, path, handler)
    }

    /// Binds a `GET` route whose handler renders an HTML page from the context alone.
    pub fn html<H, Fut>(path: impl Into<String>, handler: H) -> Self
    where
        H: Fn(HttpContext<S>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, AnyError>> + Send + 'static,
    {
        Self::bind(
            Method::Get,
            &path.into(),
            Box::new(RawHandler::new(
                InputSource::Body,
                "text/html; charset=utf-8",
                move |ctx, _input: ()| handler(ctx),
            )),
        )
    }

    /// Binds an `#[action]`-generated route whose output is JSON, its input from the query for `GET`/`DELETE`.
    #[doc(hidden)]
    pub fn action<I, O, H, Fut>(method: Method, path: impl Into<String>, handler: H) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
    {
        Self::bind(
            method,
            &path.into(),
            Box::new(TypedHandler::with_source(source_for(method), handler)),
        )
    }

    /// Binds an `#[action]`-generated route whose input is an urlencoded form body and whose output is JSON.
    #[doc(hidden)]
    pub fn action_form<I, O, H, Fut>(method: Method, path: impl Into<String>, handler: H) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
    {
        Self::bind(
            method,
            &path.into(),
            Box::new(TypedHandler::with_source(InputSource::Form, handler)),
        )
    }

    /// Binds an `#[action]`-generated route that decodes either a JSON body or a form body, whichever is declared.
    ///
    /// One route for a JSON client and a browser's own `<form>`, so the two cannot drift.
    #[doc(hidden)]
    pub fn action_body_or_form<I, O, H, Fut>(
        method: Method,
        path: impl Into<String>,
        handler: H,
    ) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, AnyError>> + Send + 'static,
    {
        Self::bind(
            method,
            &path.into(),
            Box::new(TypedHandler::with_source(InputSource::BodyOrForm, handler)),
        )
    }

    /// As [`action_body_or_form`](Self::action_body_or_form), answering its `String` output raw under `content_type`.
    pub fn action_body_or_form_raw<I, H, Fut>(
        method: Method,
        path: impl Into<String>,
        content_type: &'static str,
        handler: H,
    ) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, AnyError>> + Send + 'static,
    {
        Self::bind(
            method,
            &path.into(),
            Box::new(RawHandler::new(
                InputSource::BodyOrForm,
                content_type,
                handler,
            )),
        )
    }

    /// Binds an `#[action]`-generated route whose `String` output is returned raw under `content_type`.
    #[doc(hidden)]
    pub fn action_raw<I, H, Fut>(
        method: Method,
        path: impl Into<String>,
        content_type: &'static str,
        handler: H,
    ) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, AnyError>> + Send + 'static,
    {
        Self::bind(
            method,
            &path.into(),
            Box::new(RawHandler::new(source_for(method), content_type, handler)),
        )
    }

    /// Binds a route whose handler returns a raw `Vec<u8>` body under `content_type`.
    #[doc(hidden)]
    pub fn action_bytes<I, H, Fut>(
        method: Method,
        path: impl Into<String>,
        content_type: &'static str,
        handler: H,
    ) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, AnyError>> + Send + 'static,
    {
        Self::bind(
            method,
            &path.into(),
            Box::new(BytesHandler::new(source_for(method), content_type, handler)),
        )
    }

    /// Binds an `#[action]`-generated route that decodes a JSON or form body and answers raw bytes under `content_type`.
    #[doc(hidden)]
    pub fn action_bytes_body_or_form<I, H, Fut>(
        method: Method,
        path: impl Into<String>,
        content_type: &'static str,
        handler: H,
    ) -> Self
    where
        I: DeserializeOwned + Send + 'static,
        H: Fn(HttpContext<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, AnyError>> + Send + 'static,
    {
        Self::bind(
            method,
            &path.into(),
            Box::new(BytesHandler::new(
                InputSource::BodyOrForm,
                content_type,
                handler,
            )),
        )
    }

    /// Binds an `#[action]`-generated route that takes the raw request body as bytes and answers raw bytes.
    #[doc(hidden)]
    pub fn action_bytes_io<H, Fut>(
        method: Method,
        path: impl Into<String>,
        content_type: &'static str,
        handler: H,
    ) -> Self
    where
        H: Fn(HttpContext<S>, Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, AnyError>> + Send + 'static,
    {
        Self::bind(
            method,
            &path.into(),
            Box::new(BytesIoHandler::new(content_type, handler)),
        )
    }

    /// Binds an `#[action]`-generated route that takes the raw request body as a `String` and answers a `String`.
    #[doc(hidden)]
    pub fn action_raw_input<H, Fut>(
        method: Method,
        path: impl Into<String>,
        content_type: &'static str,
        handler: H,
    ) -> Self
    where
        H: Fn(HttpContext<S>, String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, AnyError>> + Send + 'static,
    {
        Self::bind(
            method,
            &path.into(),
            Box::new(RawInputHandler::new(content_type, handler)),
        )
    }

    /// Appends `middleware` to the route's chain; middlewares run in the order added.
    #[must_use]
    pub fn middleware<M: Middleware<S>>(mut self, middleware: M) -> Self {
        // Read the declarations off the concrete type now: one line below, `M` is erased.
        self.needs_principal |= <M as Middleware<S>>::NEEDS_PRINCIPAL;
        self.grants_authorization |= <M as Middleware<S>>::GRANTS_AUTHORIZATION;
        self.authorizes_from_path |= <M as Middleware<S>>::AUTHORIZES_FROM_PATH;
        self.reads_body |= <M as Middleware<S>>::READS_BODY;
        self.middleware.push(Box::new(middleware));
        self
    }

    /// How the backend reads this route's body.
    pub(crate) fn body_policy(&self) -> &BodyPolicy {
        &self.body_policy
    }

    /// Whether any middleware in this route's chain reads the body.
    pub(crate) fn reads_body(&self) -> bool {
        self.reads_body
    }

    /// Whether the router's declared segments are left unresolved here.
    pub(crate) fn skips_segments(&self) -> bool {
        self.skips_segments
    }

    /// Leaves the router's declared segments unresolved on this route.
    pub(crate) fn skipping_segments(mut self) -> Self {
        self.skips_segments = true;
        self
    }

    /// The method this route serves.
    pub(crate) fn method(&self) -> Method {
        self.method
    }

    /// Whether any middleware in this route's chain can only be satisfied by a principal.
    pub(crate) fn needs_principal(&self) -> bool {
        self.needs_principal
    }

    /// Declares that the act this route performs names no single object, so no object guard applies.
    #[must_use]
    pub fn no_single_object(mut self) -> Self {
        self.no_single_object = true;
        self
    }

    /// Whether any middleware in this route's chain decides which object the caller may act on.
    pub(crate) fn grants_authorization(&self) -> bool {
        self.grants_authorization
    }

    /// Declares that this route answers the origins of `set` across origins and reads no credential.
    #[must_use]
    pub fn cross_origin(mut self, set: &'static str) -> Self {
        self.cross_origin = Some(set);
        self
    }

    /// The origin set this route answers across origins, when it declared one.
    pub(crate) fn cross_origin_set(&self) -> Option<&'static str> {
        self.cross_origin
    }

    /// Whether this route declared that its act names no single object.
    pub(crate) fn declares_no_single_object(&self) -> bool {
        self.no_single_object
    }

    /// Whether an authorizing layer in this route's chain names the object the path addresses.
    pub(crate) fn authorizes_from_path(&self) -> bool {
        self.authorizes_from_path
    }

    /// Whether this route's template captures anything.
    pub(crate) fn captures_anything(&self) -> bool {
        self.pattern.captures_anything()
    }

    /// The parsed path template.
    pub(crate) fn pattern(&self) -> &RoutePattern {
        &self.pattern
    }

    /// The content type the handler's successful response carries.
    pub(crate) fn content_type(&self) -> &'static str {
        self.handler.content_type()
    }

    /// Rebuilds this route to be served on a router whose state is `Outer`, through [`FromRef`].
    ///
    /// Every declaration crosses, or a guarded write would mount as an unguarded one.
    #[must_use]
    pub fn project<Outer>(self) -> Route<Outer>
    where
        S: FromRef<Outer>,
        Outer: Send + Sync + 'static,
    {
        Route {
            method: self.method,
            pattern: self.pattern,
            middleware: Vec::new(),
            handler: Box::new(ProjectedHandler::new(self.middleware, self.handler)),
            needs_principal: self.needs_principal,
            grants_authorization: self.grants_authorization,
            no_single_object: self.no_single_object,
            authorizes_from_path: self.authorizes_from_path,
            body_policy: self.body_policy,
            reads_body: self.reads_body,
            skips_segments: self.skips_segments,
            cross_origin: self.cross_origin,
        }
    }

    /// Runs the middleware chain and handler over `context` and the raw request `body`.
    pub(crate) fn call<'a>(
        &'a self,
        context: HttpContext<S>,
        body: Vec<u8>,
    ) -> MiddlewareFuture<'a> {
        self.call_within(&[], context, body)
    }

    /// As [`call`](Self::call), with `outer`, the router's own chain, running first.
    pub(crate) fn call_within<'a>(
        &'a self,
        outer: &'a [Box<dyn ErasedMiddleware<S>>],
        context: HttpContext<S>,
        body: Vec<u8>,
    ) -> MiddlewareFuture<'a> {
        Next::new(outer, &self.middleware, self.handler.as_ref()).run(context, body)
    }

    /// As [`call_within`](Self::call_within), resolving `stage` between the router's chain and the route's.
    pub(crate) fn call_staged<'a>(
        &'a self,
        outer: &'a [Box<dyn ErasedMiddleware<S>>],
        stage: Option<SegmentStage<'a>>,
        context: HttpContext<S>,
        body: Vec<u8>,
    ) -> MiddlewareFuture<'a> {
        Next::staged(outer, &self.middleware, self.handler.as_ref(), stage).run(context, body)
    }

    /// Runs the route's middleware chain and handler in process, without a socket.
    ///
    /// # Errors
    ///
    /// Propagates any error from the middleware chain or the handler.
    pub async fn dispatch(&self, context: HttpContext<S>, body: Vec<u8>) -> MiddlewareResult {
        self.call(context, body).await
    }

    /// Runs an in-process action call at the captures and query of its address, declared segments resolved first.
    pub(crate) async fn dispatch_action(
        &self,
        context: HttpContext<S>,
        call: ActionCall,
        declared: &[Arc<dyn ErasedSegmentResolver>],
    ) -> MiddlewareResult {
        let (params, query) = match call.target.as_deref() {
            None => (PathParams::new(), ""),
            Some(target) => {
                let (path, query) = target.split_once('?').unwrap_or((target, ""));
                let params =
                    self.pattern
                        .matches(path)
                        .ok_or_else(|| ServerError::ActionPathMismatch {
                            path: path.to_owned(),
                            template: self.pattern.raw().to_owned(),
                        })?;
                (params, query)
            }
        };
        let stage = SegmentStage::for_route(declared, self);
        self.call_staged(
            &[],
            stage,
            context.for_internal_action(self.method, self.pattern.template(), params, query),
            call.body,
        )
        .await
    }
}

/// The input source an `#[action]` route reads from: the query for `GET`/`DELETE`, else the body.
fn source_for(method: Method) -> InputSource {
    match method {
        Method::Get | Method::Delete => InputSource::Query,
        _ => InputSource::Body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone)]
    struct State;

    #[derive(Deserialize)]
    struct In {
        n: u64,
    }

    #[derive(Serialize)]
    struct Out {
        doubled: u64,
    }

    #[tokio::test]
    async fn route_deserializes_input_and_serializes_output() {
        let route = Route::<State>::post("/double", |_ctx, input: In| async move {
            Ok(Out {
                doubled: input.n * 2,
            })
        });
        assert_eq!(route.method(), Method::Post);

        let ctx = HttpContext::new(State, "req-1");
        let body = route
            .call(ctx, br#"{"n":21}"#.to_vec())
            .await
            .expect("handler runs");
        assert_eq!(body, br#"{"doubled":42}"#.to_vec());
    }

    #[tokio::test]
    async fn malformed_body_is_a_deserialize_error() {
        let route = Route::<State>::post("/double", |_ctx, _input: In| async move {
            Ok(Out { doubled: 0 })
        });
        let ctx = HttpContext::new(State, "req-1");
        let err = route
            .call(ctx, br#"{"n":"oops"}"#.to_vec())
            .await
            .expect_err("bad body");
        assert_eq!(err.get().error_code(), "API_007");
        assert_eq!(err.get().http_status_code(), 400);
    }

    #[tokio::test]
    async fn auth_middleware_gates_the_route_without_a_principal() {
        let route = Route::<State>::get("/me", |_ctx, _input: ()| async move { Ok(()) })
            .middleware(crate::auth);
        let ctx = HttpContext::new(State, "req-1");
        let err = route.call(ctx, Vec::new()).await.expect_err("gated");
        assert_eq!(err.get().error_code(), "API_009");
    }

    #[test]
    fn an_ungated_route_needs_no_principal() {
        let route = Route::<State>::get("/health", |_ctx, _input: ()| async move { Ok(()) });
        assert!(!route.needs_principal());
    }

    /// A layer that decides which object may be acted on, as `PermissionGuard` does.
    struct Authorizing;

    impl Middleware<State> for Authorizing {
        const GRANTS_AUTHORIZATION: bool = true;

        fn handle(
            &self,
            ctx: HttpContext<State>,
            body: Vec<u8>,
            next: Next<'_, State>,
        ) -> impl std::future::Future<Output = MiddlewareResult> + Send {
            next.run(ctx, body)
        }
    }

    #[test]
    fn a_hand_built_route_records_that_a_guard_authorizes_an_object() {
        let route = Route::<State>::post("/revoke", |_ctx, _input: ()| async move { Ok(()) })
            .middleware(Authorizing);
        assert!(route.grants_authorization());
    }

    #[test]
    fn a_route_with_no_authorizing_layer_records_none() {
        let route = Route::<State>::post("/revoke", |_ctx, _input: ()| async move { Ok(()) })
            .middleware(crate::auth);
        assert!(!route.grants_authorization());
    }

    /// The opt-out is a statement about the act, so it is off until a route makes it.
    #[test]
    fn a_route_declares_no_single_object_only_when_it_says_so() {
        let plain = Route::<State>::post("/erase", |_ctx, _input: ()| async move { Ok(()) });
        assert!(!plain.declares_no_single_object());

        let declared = Route::<State>::post("/erase", |_ctx, _input: ()| async move { Ok(()) })
            .no_single_object();
        assert!(declared.declares_no_single_object());
    }

    // A hand-built route carries the declaration too, so the startup check cannot be side-stepped.
    #[test]
    fn a_hand_built_route_records_that_auth_needs_a_principal() {
        let route = Route::<State>::get("/me", |_ctx, _input: ()| async move { Ok(()) })
            .middleware(crate::auth);
        assert!(route.needs_principal());
    }

    #[test]
    fn a_middleware_that_can_act_anonymously_does_not_gate_the_route() {
        struct Passthrough;
        impl Middleware<State> for Passthrough {
            fn handle(
                &self,
                ctx: HttpContext<State>,
                body: Vec<u8>,
                next: Next<'_, State>,
            ) -> impl std::future::Future<Output = MiddlewareResult> + Send {
                next.run(ctx, body)
            }
        }

        let route = Route::<State>::get("/health", |_ctx, _input: ()| async move { Ok(()) })
            .middleware(Passthrough);
        assert!(!route.needs_principal());
    }

    #[test]
    fn one_gating_middleware_in_a_mixed_chain_gates_the_route() {
        struct Passthrough;
        impl Middleware<State> for Passthrough {
            fn handle(
                &self,
                ctx: HttpContext<State>,
                body: Vec<u8>,
                next: Next<'_, State>,
            ) -> impl std::future::Future<Output = MiddlewareResult> + Send {
                next.run(ctx, body)
            }
        }

        let route = Route::<State>::get("/me", |_ctx, _input: ()| async move { Ok(()) })
            .middleware(Passthrough)
            .middleware(crate::auth)
            .middleware(Passthrough);
        assert!(route.needs_principal());
    }

    #[tokio::test]
    async fn middlewares_run_in_the_order_added() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};

        struct Recorder {
            label: &'static str,
            order: Arc<AtomicUsize>,
            seen: Arc<Mutex<Vec<(&'static str, usize)>>>,
        }
        impl Middleware<State> for Recorder {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn handle(
                &self,
                ctx: HttpContext<State>,
                body: Vec<u8>,
                next: Next<'_, State>,
            ) -> impl Future<Output = crate::MiddlewareResult> + Send {
                let tick = self.order.fetch_add(1, Ordering::SeqCst);
                self.seen.lock().expect("lock").push((self.label, tick));
                next.run(ctx, body)
            }
        }

        let order = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let route = Route::<State>::get("/x", |_ctx, _input: ()| async move { Ok(()) })
            .middleware(Recorder {
                label: "first",
                order: order.clone(),
                seen: seen.clone(),
            })
            .middleware(Recorder {
                label: "second",
                order: order.clone(),
                seen: seen.clone(),
            });

        route
            .call(HttpContext::new(State, "req-1"), Vec::new())
            .await
            .expect("handler runs");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen, vec![("first", 0), ("second", 1)]);
    }
    /// A composite state, so a projection has somewhere to project *from*.
    #[derive(Clone)]
    struct Surface {
        inner: State,
    }

    impl FromRef<Surface> for State {
        fn from_ref(surface: &Surface) -> Self {
            surface.inner.clone()
        }
    }

    /// A layer that declares every one of the four things a route records about it.
    struct Declaring;

    impl Middleware<State> for Declaring {
        const NEEDS_PRINCIPAL: bool = true;
        const GRANTS_AUTHORIZATION: bool = true;
        const AUTHORIZES_FROM_PATH: bool = true;

        async fn handle(
            &self,
            ctx: HttpContext<State>,
            body: Vec<u8>,
            next: Next<'_, State>,
        ) -> MiddlewareResult {
            next.run(ctx, body).await
        }
    }

    // The router's startup checks read these four; a projection without them would hide the route from them.
    #[test]
    fn a_projected_route_still_declares_what_it_requires() {
        let route = Route::<State>::post(
            "/x",
            |_ctx, _input: In| async move { Ok(Out { doubled: 0 }) },
        )
        .middleware(Declaring)
        .no_single_object();
        assert!(route.needs_principal());
        assert!(route.grants_authorization());
        assert!(route.declares_no_single_object());
        assert!(route.authorizes_from_path());

        let projected = route.project::<Surface>();
        assert!(
            projected.needs_principal(),
            "a guarded route mounted as open"
        );
        assert!(
            projected.grants_authorization(),
            "an authorizing route mounted as one that authorizes nothing"
        );
        assert!(projected.declares_no_single_object());
        assert!(projected.authorizes_from_path());
    }

    #[test]
    fn a_projected_route_is_the_same_route() {
        let route = Route::<State>::post(
            "/organisations/{id}/members",
            |_ctx, _input: In| async move { Ok(Out { doubled: 0 }) },
        );
        let raw = route.pattern().raw().to_owned();
        let projected = route.project::<Surface>();
        assert_eq!(projected.method(), Method::Post);
        assert_eq!(projected.pattern().raw(), raw);
    }

    #[tokio::test]
    async fn a_projected_route_runs_its_own_chain_and_handler() {
        let route = Route::<State>::post("/double", |_ctx, input: In| async move {
            Ok(Out {
                doubled: input.n * 2,
            })
        })
        .middleware(Declaring)
        .project::<Surface>();

        let ctx = HttpContext::new(Surface { inner: State }, "req-projected");
        let body = route
            .call(ctx, br#"{"n":21}"#.to_vec())
            .await
            .expect("the projected chain runs");
        assert_eq!(body, br#"{"doubled":42}"#.to_vec());
    }
}
