//! `impl Surface for WsServer` — runs a WebSocket server as a supervised surface.

use std::future::pending;
use std::time::Duration;

use fundamental_core::AnyError;
use fundamental_ws::{WsListener, WsServer};

use crate::{DrainFuture, DrainPhase, StartFuture, Surface, SurfaceGuard, WaitFuture};

const NAME: &str = "socket";

impl Surface for WsServer {
    fn name(&self) -> &'static str {
        NAME
    }

    fn drain_phase(&self) -> DrainPhase {
        DrainPhase::Intake
    }

    fn drain_budget(&self) -> Option<Duration> {
        Some(WsServer::drain_budget(self))
    }

    fn start(self: Box<Self>) -> StartFuture {
        Box::pin(async move {
            let listener = self.serve().await.map_err(AnyError::new)?;
            Ok(Box::new(SocketSurfaceGuard(listener)) as Box<dyn SurfaceGuard>)
        })
    }
}

struct SocketSurfaceGuard(WsListener);

impl SurfaceGuard for SocketSurfaceGuard {
    fn name(&self) -> &'static str {
        NAME
    }

    fn wait(&mut self) -> WaitFuture<'_> {
        Box::pin(pending::<Result<(), AnyError>>())
    }

    fn stop(self: Box<Self>) -> DrainFuture {
        // Drained, not dropped: every connection is told `1001` and its handler finishes.
        Box::pin(async move { self.0.stop().await.map_err(AnyError::new) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_ws::{WsConfig, WsRouter};

    #[tokio::test]
    async fn ws_server_starts_and_stops_as_a_surface() {
        let addr = "127.0.0.1:0".parse().expect("valid addr");
        let server: Box<dyn Surface> =
            Box::new(WsServer::new(WsConfig::new(addr), WsRouter::new()));
        assert_eq!(server.name(), NAME);
        assert_eq!(server.drain_phase(), DrainPhase::Intake);
        assert_eq!(
            server.drain_budget(),
            Some(WsConfig::new(addr).drain_timeout + Duration::from_secs(1))
        );

        let guard = server.start().await.expect("binds an ephemeral port");
        assert_eq!(guard.name(), NAME);
        guard.stop().await.expect("stops cleanly");
    }
}
