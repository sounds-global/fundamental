use fundamental_ws::{WsConfig, WsListener, WsRouter};

/// Serves `router` on a port the OS picks.
pub async fn serve(router: WsRouter) -> WsListener {
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"));
    fundamental_ws::test_support::serve(config, router).await.0
}
