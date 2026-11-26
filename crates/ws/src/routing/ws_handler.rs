//! [`ErasedHandler`]: the object-safe handler the router stores, and its typed adapter.

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::context::WsContext;
use crate::error::{WsError, WsResult};

pub(crate) type HandlerFuture = Pin<Box<dyn Future<Output = WsResult<()>> + Send>>;

pub(crate) trait ErasedHandler: Send + Sync {
    fn call(&self, context: WsContext, data: Value) -> HandlerFuture;
}

/// Adapts a typed handler into an [`ErasedHandler`] by deserializing the payload into `P` first.
pub(crate) struct TypedHandler<P, H> {
    handler: H,
    _payload: PhantomData<fn(P)>,
}

impl<P, H> TypedHandler<P, H> {
    pub(crate) fn new(handler: H) -> Self {
        Self {
            handler,
            _payload: PhantomData,
        }
    }
}

impl<P, H, Fut> ErasedHandler for TypedHandler<P, H>
where
    P: DeserializeOwned + Send + 'static,
    H: Fn(WsContext, P) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = WsResult<()>> + Send + 'static,
{
    fn call(&self, context: WsContext, data: Value) -> HandlerFuture {
        match serde_json::from_value::<P>(data) {
            Ok(payload) => Box::pin((self.handler)(context, payload)),
            Err(e) => {
                let rejected = WsError::invalid_message(&e);
                Box::pin(async move { Err(rejected) })
            }
        }
    }
}
