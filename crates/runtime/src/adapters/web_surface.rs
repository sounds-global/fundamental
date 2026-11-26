//! `impl Surface for WebServer` — runs a server-rendered web server as a supervised surface.

use std::future::pending;

use fundamental_core::AnyError;
use fundamental_web::{ServerListener, WebListener, WebServer};

use crate::{DrainFuture, StartFuture, Surface, SurfaceGuard, WaitFuture};

const NAME: &str = "web";

impl<S> Surface for WebServer<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        NAME
    }

    fn start(self: Box<Self>) -> StartFuture {
        Box::pin(async move {
            let listener = self.serve().await?;
            Ok(Box::new(WebSurfaceGuard(listener)) as Box<dyn SurfaceGuard>)
        })
    }
}

struct WebSurfaceGuard(WebListener);

impl SurfaceGuard for WebSurfaceGuard {
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
    use fundamental_web::{Router, ServerConfig};

    #[tokio::test]
    async fn web_server_starts_and_stops_as_a_surface() {
        let addr = "127.0.0.1:0".parse().expect("valid addr");
        let server: Box<dyn Surface> = Box::new(WebServer::new(
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
