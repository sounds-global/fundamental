//! [`Route`]: one `(action, handler)` binding.

use std::future::Future;

use serde::de::DeserializeOwned;

use crate::context::WsContext;
use crate::error::WsResult;
use crate::routing::ws_handler::{ErasedHandler, TypedHandler};

/// A single action-to-handler binding for a [`WsRouter`](crate::WsRouter).
pub struct Route {
    pub(crate) action: String,
    pub(crate) handler: Box<dyn ErasedHandler>,
}

impl Route {
    /// Binds `action` to a handler taking the connection [`WsContext`] and a payload `P` deserialized from `data`.
    pub fn new<P, H, Fut>(action: impl Into<String>, handler: H) -> Self
    where
        P: DeserializeOwned + Send + 'static,
        H: Fn(WsContext, P) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = WsResult<()>> + Send + 'static,
    {
        Self {
            action: action.into(),
            handler: Box::new(TypedHandler::new(handler)),
        }
    }
}
