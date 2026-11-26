use std::{
    any::TypeId, collections::HashMap, fmt, future::Future, pin::Pin, sync::Arc, sync::Mutex,
};

use crate::action_call::ActionCall;
use crate::segment_resolver::ErasedSegmentResolver;
use crate::{HttpContext, MiddlewareResult, PathShape, ServerError};

type DispatchFuture = Pin<Box<dyn Future<Output = MiddlewareResult> + Send + 'static>>;
/// The router's declared segment resolvers, shared with each in-process call.
pub(crate) type DeclaredSegments = Arc<[Arc<dyn ErasedSegmentResolver>]>;
type RouteCall<S> =
    dyn Fn(HttpContext<S>, ActionCall, DeclaredSegments) -> DispatchFuture + Send + Sync;

/// Per-request access to the mounted action routes during SSR, middleware included.
#[derive(Clone)]
pub struct ActionDispatcher {
    call: Arc<dyn Fn(TypeId, ActionCall) -> DispatchFuture + Send + Sync>,
}

impl fmt::Debug for ActionDispatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionDispatcher").finish_non_exhaustive()
    }
}

impl ActionDispatcher {
    pub(crate) fn new(
        call: impl Fn(TypeId, ActionCall) -> DispatchFuture + Send + Sync + 'static,
    ) -> Self {
        Self {
            call: Arc::new(call),
        }
    }

    /// Invokes a mounted action by its marker type, with no captures and no query.
    ///
    /// # Errors
    /// Returns the action route's error, or a server error when the action was not mounted.
    pub async fn dispatch<M: 'static>(&self, body: Vec<u8>) -> MiddlewareResult {
        let call = ActionCall { target: None, body };
        (self.call)(TypeId::of::<M>(), call).await
    }

    /// Invokes a mounted action at `target`, its own path with an optional `?query`.
    ///
    /// # Errors
    /// [`ServerError::ActionPathMismatch`] when `target`'s path is not one the action's route
    /// serves; otherwise as [`dispatch`](Self::dispatch).
    pub async fn dispatch_at<M: 'static>(&self, target: &str, body: Vec<u8>) -> MiddlewareResult {
        let call = ActionCall {
            target: Some(target.to_owned()),
            body,
        };
        (self.call)(TypeId::of::<M>(), call).await
    }
}

pub(crate) struct ActionRoutes<S> {
    routes: Mutex<HashMap<TypeId, Arc<RouteCall<S>>>>,
    segments: Mutex<Vec<Arc<dyn ErasedSegmentResolver>>>,
}

impl<S> fmt::Debug for ActionRoutes<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self
            .routes
            .lock()
            .map(|routes| routes.len())
            .unwrap_or_default();
        f.debug_struct("ActionRoutes")
            .field("route_count", &count)
            .finish()
    }
}

impl<S> Default for ActionRoutes<S> {
    fn default() -> Self {
        Self {
            routes: Mutex::new(HashMap::new()),
            segments: Mutex::new(Vec::new()),
        }
    }
}

impl<S: Send + Sync + 'static> ActionRoutes<S> {
    pub(crate) fn insert<M: 'static>(
        self: &Arc<Self>,
        call: impl Fn(HttpContext<S>, ActionCall, DeclaredSegments) -> DispatchFuture
        + Send
        + Sync
        + 'static,
    ) {
        self.routes
            .lock()
            .expect("action routes mutex is not poisoned")
            .insert(TypeId::of::<M>(), Arc::new(call));
    }

    /// Declares a segment resolver the router declared, for the routes it dispatches.
    pub(crate) fn declare_segment(&self, resolver: Arc<dyn ErasedSegmentResolver>) {
        self.segments
            .lock()
            .expect("action segments mutex is not poisoned")
            .push(resolver);
    }
}

impl<S: Clone + Send + Sync + 'static> ActionRoutes<S> {
    pub(crate) fn for_request(self: &Arc<Self>, context: HttpContext<S>) -> ActionDispatcher {
        let routes = Arc::clone(self);
        let context = Arc::new(context);
        let segments: DeclaredSegments = self
            .segments
            .lock()
            .expect("action segments mutex is not poisoned")
            .iter()
            .cloned()
            .collect();
        ActionDispatcher::new(move |marker, call| {
            let route = routes
                .routes
                .lock()
                .expect("action routes mutex is not poisoned")
                .get(&marker)
                .cloned();
            match route {
                Some(route) => route((*context).clone(), call, Arc::clone(&segments)),
                None => Box::pin(async {
                    Err(ServerError::NotFound {
                        method: "SSR".to_owned(),
                        path: PathShape::new("mounted SSR action", |_| true),
                    }
                    .into())
                }),
            }
        })
    }
}
