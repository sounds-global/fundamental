//! [`WsServer`]: binds a listener and serves each connection through a router.

use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpListener, TcpSocket};
use tokio::sync::oneshot;
use tokio::time::Instant;
use tracing::{Instrument, error, info, info_span};

use crate::config::WsConfig;
use crate::error::{WsError, WsResult};
use crate::observability::{WsGauges, WsStats};
use crate::routing::WsRouter;
use crate::server::WsListener;
use crate::server::accept_loop::accept_loop;
use crate::session::CLOSE_GRACE;
use crate::slots::AdmissionControl;

/// A WebSocket server: it runs every accepted connection's session against a shared [`WsRouter`]
/// within the bounds its [`WsConfig`] declares.
pub struct WsServer {
    config: Arc<WsConfig>,
    router: Arc<WsRouter>,
    control: Arc<AdmissionControl>,
    listener: Option<TcpListener>,
}

impl WsServer {
    /// Creates a server that will serve `router` under `config`.
    #[must_use]
    pub fn new(config: WsConfig, router: WsRouter) -> Self {
        let control = AdmissionControl::new(&config, Instant::now());
        Self {
            config: Arc::new(config),
            router: Arc::new(router),
            control,
            listener: None,
        }
    }

    /// Serves on `listener` (a socket-activated one, say) instead of binding [`WsConfig::bind`];
    /// [`WsConfig::accept_backlog`] does not apply to it.
    #[must_use]
    pub fn with_listener(mut self, listener: TcpListener) -> Self {
        self.listener = Some(listener);
        self
    }

    /// The server's connection and close counters, shared with every session it runs.
    #[must_use]
    pub fn stats(&self) -> Arc<WsStats> {
        self.control.shared_stats()
    }

    /// The longest [`WsListener::stop`] can take: the drain plus one close grace.
    #[must_use]
    pub fn drain_budget(&self) -> Duration {
        self.config.drain_timeout.saturating_add(CLOSE_GRACE)
    }

    /// A Prometheus collector over [`stats`](Self::stats), unregistered, its names prefixed by `namespace`.
    ///
    /// # Errors
    /// Returns [`prometheus::Error`] if `namespace` makes a metric name invalid.
    pub fn gauges(&self, namespace: &str) -> Result<WsGauges, prometheus::Error> {
        WsGauges::new(namespace, self.stats())
    }

    /// Validates the configuration, binds the listener and spawns the accept loop.
    ///
    /// [`WsListener::stop`] drains the server; dropping the guard aborts it.
    ///
    /// # Errors
    /// - [`WsError::InvalidConfig`] if the configuration breaks a rule, naming every one.
    /// - [`WsError::Connection`] if the configured address cannot be bound, or the listener
    ///   cannot report its address.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub async fn serve(self) -> WsResult<WsListener> {
        if let Err(error) = self.config.validate() {
            error!(%error, "websocket configuration refused");
            return Err(error);
        }
        self.config.log_bounds();
        let Self {
            config,
            router,
            control,
            listener,
        } = self;
        let listener = match listener {
            Some(listener) => listener,
            None => listen(&config).map_err(|e| {
                error!(error = %e, addr = %config.bind, "websocket listener not bound");
                WsError::connection(e.to_string())
            })?,
        };
        let local_addr = listener.local_addr().map_err(|e| {
            error!(error = %e, "websocket listener has no local address");
            WsError::connection(e.to_string())
        })?;

        info!(addr = %local_addr, "websocket server listening");
        let (stop, stopped) = oneshot::channel();
        // The span names the server, so two drained at once can be told apart.
        let handle = tokio::spawn(
            accept_loop(listener, config, router, Arc::clone(&control), stopped)
                .instrument(info_span!("ws.server", addr = %local_addr)),
        );
        Ok(WsListener::new(handle, stop, local_addr, control))
    }
}

/// Built by hand because `TcpListener::bind` asks for a 1024 queue, which a fleet reconnecting at once overruns.
fn listen(config: &WsConfig) -> std::io::Result<TcpListener> {
    let socket = if config.bind.is_ipv4() {
        TcpSocket::new_v4()?
    } else {
        TcpSocket::new_v6()?
    };
    socket.set_reuseaddr(true)?;
    socket.bind(config.bind)?;
    socket.listen(config.accept_backlog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::WsRouter;
    use fundamental_core::Error as _;

    fn addr() -> std::net::SocketAddr {
        "127.0.0.1:0".parse().expect("valid addr")
    }

    #[tokio::test]
    async fn serve_binds_an_ephemeral_port() {
        let server = WsServer::new(WsConfig::new(addr()), WsRouter::new());
        let listener = server.serve().await.expect("serve binds");
        assert_ne!(listener.local_addr().port(), 0);
    }

    #[tokio::test]
    async fn an_invalid_configuration_is_refused_before_binding() {
        let server = WsServer::new(WsConfig::new(addr()).max_connections(0), WsRouter::new());
        let Err(error) = server.serve().await else {
            panic!("an invalid configuration served");
        };
        assert_eq!(error.error_code(), "WS_010");
    }

    #[tokio::test]
    async fn a_listener_bound_before_the_server_is_served() {
        let bound = TcpListener::bind(addr()).await.expect("bound");
        let expected = bound.local_addr().expect("an address");
        let server = WsServer::new(WsConfig::new(addr()), WsRouter::new()).with_listener(bound);
        let listener = server.serve().await.expect("serves the listener");
        assert_eq!(listener.local_addr(), expected);
    }

    #[test]
    fn the_drain_budget_covers_the_drain_and_the_aborts() {
        let config = WsConfig::new(addr()).drain_timeout(Duration::from_secs(12));
        let server = WsServer::new(config, WsRouter::new());
        assert_eq!(server.drain_budget(), Duration::from_secs(12) + CLOSE_GRACE);
    }

    #[test]
    fn gauges_are_built_before_serving() {
        let server = WsServer::new(WsConfig::new(addr()), WsRouter::new());
        assert!(server.gauges("app").is_ok());
        assert_eq!(server.stats().anonymous(), 0);
    }
}
