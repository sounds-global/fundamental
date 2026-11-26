use crate::{Route, ServerRequest};

/// A router's error page: its route, and how a request becomes the one it renders with.
///
/// A function pointer, so `S: Clone` is needed only where the page is registered.
pub(crate) struct ErrorPage<S> {
    pub(crate) route: Route<S>,
    pub(crate) request_for: fn(&ServerRequest<S>) -> ServerRequest<S>,
}
