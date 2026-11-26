use tracing::{info, instrument};

use crate::client::NatsClient;
use crate::error::{NatsError, NatsResult};
use crate::subscription::Listener;
use crate::worker_guard::WorkerGuard;

/// A NATS run-loop owning a client and its subscription guards.
#[must_use = "a Worker does nothing until `run` is awaited"]
#[derive(Debug)]
pub struct Worker {
    // `None` only for the test-only `from_listeners`.
    client: Option<NatsClient>,
    listeners: Vec<Listener>,
}

impl Worker {
    /// Creates a worker that owns `client` and, initially, no listeners.
    pub fn new(client: NatsClient) -> Self {
        Self {
            client: Some(client),
            listeners: Vec::new(),
        }
    }

    /// Adds one subscription guard to keep alive for the worker's lifetime.
    pub fn with_listener(mut self, listener: Listener) -> Self {
        self.listeners.push(listener);
        self
    }

    /// Adds every subscription guard from `listeners`.
    pub fn with_listeners(mut self, listeners: impl IntoIterator<Item = Listener>) -> Self {
        self.listeners.extend(listeners);
        self
    }

    /// Packages the client and its listeners into a running [`WorkerGuard`].
    ///
    /// The listeners are already dispatching; dropping the guard stops them and closes the
    /// connection.
    pub fn serve(self) -> WorkerGuard {
        let Self { client, listeners } = self;
        info!(listeners = listeners.len(), "worker serving");
        WorkerGuard::new(client, listeners)
    }

    /// Runs until an OS shutdown signal arrives, then drains every listener.
    ///
    /// For a single-surface `main`; under other surfaces use `fundamental_runtime::Runtime`.
    ///
    /// # Errors
    /// Returns [`NatsError::Signal`] if the shutdown-signal handler cannot be
    /// registered or awaited, or the error a listener's drain reported.
    #[instrument(skip(self), fields(listeners = self.listeners.len()), err(Debug))]
    pub async fn run(self) -> NatsResult<()> {
        let guard = self.serve();

        tokio::signal::ctrl_c()
            .await
            .map_err(|source| NatsError::Signal { source })?;

        info!("shutdown signal received");
        guard.drain().await
    }

    /// The number of listeners currently attached.
    #[cfg(test)]
    fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    /// Builds a client-less worker for builder-accounting tests.
    #[cfg(test)]
    fn from_listeners(listeners: impl IntoIterator<Item = Listener>) -> Self {
        Self {
            client: None,
            listeners: listeners.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `run()` blocks on `ctrl_c`, so only the builder is tested.
    fn dummy_listener() -> Listener {
        Listener::spawn(|_| std::future::pending())
    }

    #[tokio::test]
    async fn new_worker_has_no_listeners() {
        let worker = Worker::from_listeners(std::iter::empty());
        assert_eq!(worker.listener_count(), 0);
    }

    #[tokio::test]
    async fn with_listener_adds_one_each() {
        let worker = Worker::from_listeners(std::iter::empty())
            .with_listener(dummy_listener())
            .with_listener(dummy_listener());
        assert_eq!(worker.listener_count(), 2);
    }

    #[tokio::test]
    async fn with_listeners_adds_each() {
        let listeners = vec![dummy_listener(), dummy_listener(), dummy_listener()];
        let worker = Worker::from_listeners(listeners);
        assert_eq!(worker.listener_count(), 3);
    }

    #[tokio::test]
    async fn with_listeners_extends_existing() {
        let worker = Worker::from_listeners(vec![dummy_listener()])
            .with_listeners(vec![dummy_listener(), dummy_listener()]);
        assert_eq!(worker.listener_count(), 3);
    }

    #[tokio::test]
    async fn serve_hands_every_listener_to_the_guard() {
        let worker =
            Worker::from_listeners(vec![dummy_listener(), dummy_listener(), dummy_listener()]);
        let guard = worker.serve();
        assert_eq!(guard.listener_count(), 3);
    }
}
