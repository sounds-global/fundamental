//! A stop is seen even while the accept loop waits for a slot on a full server.

use fundamental_observability::testing::CapturedLogs;
use fundamental_ws::test_support::{close_frame, connect, serve, until};
use fundamental_ws::{CloseReason, WsConfig, WsRouter};
use tokio::net::TcpStream;
use tokio::time::timeout;

#[tokio::test]
async fn a_stop_on_a_full_server_completes_within_its_drain() {
    let captured = CapturedLogs::install().expect("the only subscriber");
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"))
        .public()
        .max_connections(1);
    let drain = config.drain_timeout;
    let (listener, stats) = serve(config, WsRouter::new()).await;
    let mut holder = connect(&listener).await;
    until(|| stats.public() == 1, "the one slot is held").await;
    let _waiting = TcpStream::connect(listener.local_addr())
        .await
        .expect("the kernel accepts the connection");
    until(
        || {
            captured
                .text()
                .contains("every public slot is taken; waiting for one")
        },
        "the server waits for a slot",
    )
    .await;

    timeout(drain, listener.stop())
        .await
        .expect("the stop was seen while the server waited for a slot")
        .expect("the server drains");

    assert_eq!(
        close_frame(&mut holder).await,
        (1001, "going_away".to_string())
    );
    assert_eq!(stats.closes(CloseReason::GoingAway), 1);
    let written = captured.text();
    let finished = written
        .lines()
        .find(|line| line.contains("websocket drain finished"))
        .expect("the drain wrote its line");
    for field in ["open=1", "going_away=1", "finished=1", "aborted=0"] {
        assert!(finished.contains(field), "{field} missing from: {finished}");
    }
    assert!(!written.contains("websocket drain timed out"));
}
