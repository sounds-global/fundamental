//! [`LiveSyncSurface`]: a WebSocket slice-sync server that owns its background work.

use std::future::pending;
use std::time::Duration;

use fundamental_core::AnyError;
use fundamental_ws::{WsListener, WsServer};
use tokio::task::JoinHandle;
use tracing::debug;

use crate::{DrainFuture, DrainPhase, StartFuture, Surface, SurfaceGuard, WaitFuture};

const NAME: &str = "sync";

/// A live slice-sync [`WsServer`] run as a supervised [`Surface`], owning the change-capture handles, the
/// change publisher and the projection task, and dropping them when drained.
#[must_use = "a LiveSyncSurface does nothing until handed to a Runtime"]
pub struct LiveSyncSurface {
    server: WsServer,
    keep_alive: Vec<Box<dyn Send>>,
    projection: Option<JoinHandle<()>>,
}

impl LiveSyncSurface {
    /// Wraps `server` with the background work it depends on.
    ///
    /// Order `keep_alive` capture handles first, publisher last, so capture stops before the publisher closes.
    pub fn new(
        server: WsServer,
        keep_alive: Vec<Box<dyn Send>>,
        projection: Option<JoinHandle<()>>,
    ) -> Self {
        Self {
            server,
            keep_alive,
            projection,
        }
    }
}

impl Surface for LiveSyncSurface {
    fn name(&self) -> &'static str {
        NAME
    }

    fn drain_phase(&self) -> DrainPhase {
        DrainPhase::Intake
    }

    fn drain_budget(&self) -> Option<Duration> {
        Some(self.server.drain_budget())
    }

    fn start(self: Box<Self>) -> StartFuture {
        Box::pin(async move {
            let listener = self.server.serve().await.map_err(AnyError::new)?;
            Ok(Box::new(LiveSyncGuard {
                listener,
                keep_alive: self.keep_alive,
                projection: self.projection,
            }) as Box<dyn SurfaceGuard>)
        })
    }
}

struct LiveSyncGuard {
    listener: WsListener,
    keep_alive: Vec<Box<dyn Send>>,
    projection: Option<JoinHandle<()>>,
}

impl SurfaceGuard for LiveSyncGuard {
    fn name(&self) -> &'static str {
        NAME
    }

    fn wait(&mut self) -> WaitFuture<'_> {
        Box::pin(pending::<Result<(), AnyError>>())
    }

    fn stop(self: Box<Self>) -> DrainFuture {
        Box::pin(async move {
            // Drain connections while capture still runs, so a last frame still has its changes.
            let drained = self.listener.stop().await.map_err(AnyError::new);
            if let Some(projection) = self.projection {
                projection.abort();
            }
            drop(self.keep_alive);
            debug!("live sync surface stopped");
            drained
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_ws::{WsConfig, WsRouter};

    #[tokio::test]
    async fn live_sync_surface_starts_and_stops_owning_its_handles() {
        struct DropFlag(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropFlag {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let addr = "127.0.0.1:0".parse().expect("valid addr");
        let surface: Box<dyn Surface> = Box::new(LiveSyncSurface::new(
            WsServer::new(WsConfig::new(addr), WsRouter::new()),
            vec![Box::new(DropFlag(dropped.clone()))],
            None,
        ));
        assert_eq!(surface.name(), NAME);

        let guard = surface.start().await.expect("binds an ephemeral port");
        assert_eq!(guard.name(), NAME);
        assert!(
            !dropped.load(std::sync::atomic::Ordering::SeqCst),
            "capture handle stays alive while the surface runs"
        );

        guard.stop().await.expect("stops cleanly");
        assert!(
            dropped.load(std::sync::atomic::Ordering::SeqCst),
            "draining the surface drops its capture handles"
        );
    }
}
