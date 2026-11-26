//! `impl Surface for ApiServer` — runs an HTTP API server as a supervised surface.

use std::future::pending;

use fundamental_api::{ApiServer, ServerListener};
use fundamental_core::AnyError;

use crate::{DrainFuture, DrainPhase, StartFuture, Surface, SurfaceGuard, WaitFuture};

const NAME: &str = "api";

impl<S> Surface for ApiServer<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        NAME
    }

    /// An HTTP listener is the front door, so it closes first (the admin listener overrides this).
    fn drain_phase(&self) -> DrainPhase {
        DrainPhase::Intake
    }

    fn start(self: Box<Self>) -> StartFuture {
        Box::pin(async move {
            let listener = self.serve().await?;
            Ok(Box::new(ApiSurfaceGuard(listener)) as Box<dyn SurfaceGuard>)
        })
    }
}

struct ApiSurfaceGuard(fundamental_api::ApiListener);

impl SurfaceGuard for ApiSurfaceGuard {
    fn name(&self) -> &'static str {
        NAME
    }

    fn wait(&mut self) -> WaitFuture<'_> {
        Box::pin(pending::<Result<(), AnyError>>())
    }

    fn stop(self: Box<Self>) -> DrainFuture {
        Box::pin(async move { self.0.stop().await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_api::{Router, ServerConfig};

    #[tokio::test]
    async fn api_server_starts_and_stops_as_a_surface() {
        let addr = "127.0.0.1:0".parse().expect("valid addr");
        let server: Box<dyn Surface> = Box::new(ApiServer::new(
            ServerConfig::new(addr),
            (),
            Router::<()>::new(),
        ));
        assert_eq!(server.name(), NAME);

        let guard = server.start().await.expect("binds an ephemeral port");
        assert_eq!(guard.name(), NAME);
        guard.stop().await.expect("stops cleanly");
    }
}
