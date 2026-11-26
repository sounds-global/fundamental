use std::sync::Arc;

use fundamental_core::AnyError;

use crate::Router;
use crate::server_backend::ServerBackend;
use crate::server_config::ServerConfig;

/// A server: a [`Router`] and its application state, bound to a pluggable [`ServerBackend`].
pub struct Server<B, S> {
    backend: B,
    config: ServerConfig,
    state: S,
    router: Arc<Router<S>>,
}

impl<B, S> Server<B, S>
where
    B: ServerBackend,
    S: Clone + Send + Sync + 'static,
{
    /// Creates a server that will serve `router` over `state` using `backend`.
    pub fn new(backend: B, config: ServerConfig, state: S, router: Router<S>) -> Self {
        Self {
            backend,
            config,
            state,
            router: Arc::new(router),
        }
    }

    /// Binds the configured address and begins serving, returning the backend's running-server handle.
    ///
    /// # Errors
    /// Returns an [`AnyError`] if the backend cannot bind the configured address.
    pub async fn serve(self) -> Result<B::Listener, AnyError> {
        self.backend
            .serve(self.config, self.state, self.router)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    use fundamental_core::listen::Listen;

    use crate::server_backend::ServerListener;

    #[derive(Clone)]
    struct State;

    struct FakeListener {
        listen: Listen,
    }

    impl ServerListener for FakeListener {
        fn local_addr(&self) -> &Listen {
            &self.listen
        }

        async fn stop(self) -> Result<(), AnyError> {
            Ok(())
        }
    }

    struct FakeBackend;

    impl ServerBackend for FakeBackend {
        type Listener = FakeListener;

        async fn serve<S>(
            &self,
            config: ServerConfig,
            _state: S,
            _router: Arc<Router<S>>,
        ) -> Result<Self::Listener, AnyError>
        where
            S: Clone + Send + Sync + 'static,
        {
            Ok(FakeListener {
                listen: config.listen().clone(),
            })
        }
    }

    #[tokio::test]
    async fn serve_delegates_to_the_backend() {
        let addr: SocketAddr = "127.0.0.1:9000".parse().expect("addr");
        let server = Server::new(FakeBackend, ServerConfig::new(addr), State, Router::new());

        let listener = server.serve().await.expect("fake backend binds");
        assert_eq!(listener.local_addr(), &Listen::Address(addr));
        listener.stop().await.expect("stops cleanly");
    }
}
