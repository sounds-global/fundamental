//! A fault run's own NATS server stops, refuses clients while stopped, and accepts them again
//! once started.

#![cfg(feature = "nats")]

use fundamental_nats::{NatsClient, NatsConfig, ReconnectConfig};
use fundamental_test_containers::nats::DedicatedNatsServer;

fn config(url: &str) -> NatsConfig {
    NatsConfig::new(url).with_reconnect(ReconnectConfig::fail_fast())
}

#[tokio::test]
async fn a_stopped_server_refuses_clients_until_started_again() {
    let mut server = DedicatedNatsServer::start()
        .await
        .expect("the fault server");
    let before = server.url();
    server.stop().await.expect("stopped");
    assert!(
        NatsClient::connect(config(&before)).await.is_err(),
        "a stopped server refuses"
    );
    server.start_again().await.expect("started again");
    NatsClient::connect(config(&server.url()))
        .await
        .expect("the server accepts again");
}
