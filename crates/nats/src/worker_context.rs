use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use fundamental_core::AnyError;

use crate::{Listener, NatsClient};

/// The future a `#[subscribe]` handler's `{fn}_worker` returns: its listener, started.
pub type ListenerFuture<'a> = Pin<Box<dyn Future<Output = Result<Listener, AnyError>> + Send + 'a>>;

/// The future `mount_workers!`'s `register_workers` returns: every listed handler's listener.
pub type WorkersFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<Listener>, AnyError>> + Send + 'a>>;

/// What every `#[subscribe]` handler of a worker is subscribed with: the client, its JetStream
/// context, and provided values such as an `Inbox`.
#[derive(Clone)]
pub struct WorkerContext {
    client: NatsClient,
    provided: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl WorkerContext {
    /// The context of a worker connected through `client`, providing nothing else yet.
    #[must_use]
    pub fn new(client: NatsClient) -> Self {
        Self {
            client,
            provided: HashMap::new(),
        }
    }

    /// Provides `value` to every handler that asks for its type, replacing one given before.
    #[must_use]
    pub fn provide<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.provided.insert(TypeId::of::<T>(), Arc::new(value));
        self
    }

    /// The value of type `T` this context was [provided](Self::provide), if any.
    #[must_use]
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.provided
            .get(&TypeId::of::<T>())
            .and_then(|value| value.downcast_ref::<T>())
    }

    /// The connected client a core-NATS handler subscribes through.
    #[must_use]
    pub fn client(&self) -> &NatsClient {
        &self.client
    }

    /// The JetStream context a durable handler consumes through.
    #[cfg(feature = "jetstream")]
    #[must_use]
    pub fn jetstream(&self) -> crate::JetStream {
        self.client.jetstream()
    }

    /// The client, for the [`Worker`](crate::Worker) that owns the listeners.
    #[must_use]
    pub fn into_client(self) -> NatsClient {
        self.client
    }
}

impl std::fmt::Debug for WorkerContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerContext")
            .field("provided", &self.provided.len())
            .finish_non_exhaustive()
    }
}
