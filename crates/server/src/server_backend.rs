use std::future::Future;
use std::sync::Arc;

use fundamental_core::AnyError;
use fundamental_core::listen::Listen;

use crate::Router;
use crate::server_config::ServerConfig;

/// A pluggable HTTP server backend.
pub trait ServerBackend: Send + Sync + 'static {
    /// The running-server handle this backend returns.
    type Listener: ServerListener;

    /// Binds the configured address and begins serving `router` over `state`.
    ///
    /// # Errors
    /// Returns an [`AnyError`] (typically wrapping
    /// [`ServerError::Bind`](crate::ServerError::Bind)) if the address cannot be
    /// bound.
    fn serve<S>(
        &self,
        config: ServerConfig,
        state: S,
        router: Arc<Router<S>>,
    ) -> impl Future<Output = Result<Self::Listener, AnyError>> + Send
    where
        S: Clone + Send + Sync + 'static;
}

/// A handle to a running server, used to observe its address and stop it.
pub trait ServerListener: Send + 'static {
    /// Where the server actually listens: the bound address or the passed socket.
    fn local_addr(&self) -> &Listen;

    /// Stops the server, draining in-flight work.
    ///
    /// # Errors
    /// Returns an [`AnyError`] if shutdown could not complete cleanly.
    fn stop(self) -> impl Future<Output = Result<(), AnyError>> + Send;
}
