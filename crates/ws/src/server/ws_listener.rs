//! [`WsListener`]: a guard over a running [`WsServer`](crate::WsServer) accept loop.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::{debug, error};

use crate::error::{WsError, WsResult};
use crate::observability::WsStats;
use crate::slots::AdmissionControl;

/// A running WebSocket server; dropping it without [`stop`](Self::stop) aborts every session unannounced.
#[must_use = "dropping the WsListener immediately stops the server"]
#[derive(Debug)]
pub struct WsListener {
    handle: Option<JoinHandle<()>>,
    stop: Option<oneshot::Sender<()>>,
    local_addr: SocketAddr,
    control: Arc<AdmissionControl>,
}

impl WsListener {
    pub(crate) fn new(
        handle: JoinHandle<()>,
        stop: oneshot::Sender<()>,
        local_addr: SocketAddr,
        control: Arc<AdmissionControl>,
    ) -> Self {
        Self {
            handle: Some(handle),
            stop: Some(stop),
            local_addr,
            control,
        }
    }

    /// The server's connection and close counters.
    #[must_use]
    pub fn stats(&self) -> Arc<WsStats> {
        self.control.shared_stats()
    }

    /// The address the server is bound to.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Drains the server: the listener closes, then each connection ends at its next frame boundary
    /// with `1001`; those still running at [`drain_timeout`](crate::WsConfig::drain_timeout) are aborted.
    ///
    /// # Errors
    /// Returns [`WsError::Internal`] if the accept-loop task panicked.
    pub async fn stop(mut self) -> WsResult<()> {
        if let Some(stop) = self.stop.take()
            && stop.send(()).is_err()
        {
            debug!("the accept loop had already ended before the stop");
        }
        self.join_loop().await
    }

    /// Awaits the accept loop's completion.
    ///
    /// # Errors
    /// Returns [`WsError::Internal`] if the accept-loop task panicked.
    pub async fn join(mut self) -> WsResult<()> {
        self.join_loop().await
    }

    /// Keeps the handle while awaiting, so a caller that gives up drops a listener whose [`Drop`] aborts the loop.
    async fn join_loop(&mut self) -> WsResult<()> {
        let Some(handle) = self.handle.as_mut() else {
            return Ok(());
        };
        let joined = handle.await;
        self.handle = None;
        joined.map_err(|e| {
            error!(error = %e, "websocket accept loop panicked");
            WsError::internal(format!("server task panicked: {e}"))
        })
    }
}

impl Drop for WsListener {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            // Each aborted session counts as cut short by the stop, not as failed.
            self.control.drain().expire();
            handle.abort();
        }
    }
}
