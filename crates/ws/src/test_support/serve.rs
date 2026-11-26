//! [`serve`] — a server on a loopback port the OS picks.

use std::sync::Arc;

use crate::{WsConfig, WsListener, WsRouter, WsServer, WsStats};

/// Serves `router` under `config` on `127.0.0.1:0`, returning the listener and its counters.
///
/// # Panics
/// If the server cannot be served.
pub async fn serve(config: WsConfig, router: WsRouter) -> (WsListener, Arc<WsStats>) {
    let config = WsConfig {
        bind: "127.0.0.1:0".parse().expect("a loopback address"),
        ..config
    };
    let listener = WsServer::new(config, router)
        .serve()
        .await
        .expect("the server serves");
    let stats = listener.stats();
    (listener, stats)
}
