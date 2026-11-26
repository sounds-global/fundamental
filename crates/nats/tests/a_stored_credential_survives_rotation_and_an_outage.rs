//! A stored password is refreshed in the background: a rotation reaches the next reconnect, and
//! with the store down the value last read is used.

mod support;

use std::net::SocketAddr;
use std::time::Duration;

use fundamental_core::Environment;
use fundamental_nats::{ConnectionState, NatsClient, NatsConfig, ReconnectConfig};
use fundamental_test_containers::TcpProxy;
use fundamental_test_containers::nats::{NatsAuthServer, NatsUser};
use serde_json::json;
use support::{CapturedLog, eventually, nats_settings, openbao, unique, vault_at};
use tokio::sync::broadcast;

/// Long enough for a one-second refresh and a reconnect on a short backoff, on a loaded machine.
const BUDGET: Duration = Duration::from_secs(20);

/// Reconnects quickly and forever, so a test waits on the credential rather than the backoff.
fn quick_reconnects() -> ReconnectConfig {
    ReconnectConfig::default()
        .with_initial_delay(Duration::from_millis(100))
        .with_max_delay(Duration::from_millis(500))
}

/// Waits for an event on `events` that `wanted` accepts.
async fn next_event(
    events: &mut broadcast::Receiver<fundamental_nats::ConnectionEvent>,
    what: &str,
    wanted: impl Fn(&fundamental_nats::ConnectionEvent) -> bool,
) {
    let deadline = tokio::time::Instant::now() + BUDGET;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(event)) if wanted(&event) => return,
            Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                panic!("the client closed before {what}")
            }
            Err(_) => panic!("{what} did not happen within {BUDGET:?}"),
        }
    }
}

#[tokio::test]
async fn a_rotated_password_is_used_after_the_next_reconnect() {
    let (log, _guard) = CapturedLog::install();
    let bao = openbao().await;
    let path = unique("rotated");
    let right = NatsUser::App.password();
    bao.kv_put(&path, &json!({ "password": right.expose() }))
        .await
        .expect("the password is stored");
    let server = NatsAuthServer::shared().await.expect("the auth server");
    let settings = nats_settings(&format!(
        "url = \"{}\"\nusername = \"{}\"\nsecret_refresh_seconds = 1\n\
         password = {{ source = \"vault_kv\", mount = \"secret\", path = \"{path}\", key = \"password\" }}\n",
        server.url(),
        NatsUser::App.username(),
    ));
    let vault = vault_at(bao.address()).await;
    let config = NatsConfig::from_settings(&settings, Environment::Development, Some(&vault))
        .await
        .expect("the reference is read");
    let client = NatsClient::connect(config.with_reconnect(quick_reconnects()))
        .await
        .expect("connects with the stored password");
    client.flush().await.expect("answers");
    let mut events = client.subscribe_events();

    // Rotated away from the server's fixed password, the next connection is refused.
    bao.kv_put(&path, &json!({ "password": "rotated-away" }))
        .await
        .expect("rotated");
    log.wait_for("secret rotated", BUDGET).await;
    client.force_reconnect().await.expect("reconnect requested");
    next_event(&mut events, "a refused reconnect", |event| {
        event
            .error
            .as_deref()
            .is_some_and(|error| error.to_lowercase().contains("authoriz"))
    })
    .await;

    // Rotated back, the running client reconnects on its own.
    bao.kv_put(&path, &json!({ "password": right.expose() }))
        .await
        .expect("rotated back");
    eventually("a second rotation", BUDGET, || {
        log.count("secret rotated") >= 2
    })
    .await;
    eventually("the reconnect", BUDGET, || client.is_connected()).await;
    client
        .flush()
        .await
        .expect("answers on the rotated password");
    assert_eq!(client.state(), ConnectionState::Connected);
    assert!(
        !log.text().contains(right.expose()),
        "the password was logged"
    );
}

#[tokio::test]
async fn a_reconnect_with_the_secret_store_down_uses_the_cached_value() {
    let (log, _guard) = CapturedLog::install();
    let bao = openbao().await;
    let path = unique("outage");
    let password = NatsUser::App.password();
    bao.kv_put(&path, &json!({ "password": password.expose() }))
        .await
        .expect("the password is stored");
    let upstream: SocketAddr = bao
        .address()
        .trim_start_matches("http://")
        .parse()
        .expect("a loopback address");
    let proxy = TcpProxy::start(upstream).await.expect("a proxy");
    let vault = vault_at(&format!("http://{}", proxy.address())).await;
    let server = NatsAuthServer::shared().await.expect("the auth server");
    let settings = nats_settings(&format!(
        "url = \"{}\"\nusername = \"{}\"\nsecret_refresh_seconds = 1\n\
         password = {{ source = \"vault_kv\", mount = \"secret\", path = \"{path}\", key = \"password\" }}\n",
        server.url(),
        NatsUser::App.username(),
    ));
    let config = NatsConfig::from_settings(&settings, Environment::Development, Some(&vault))
        .await
        .expect("the reference is read while the store is up");
    let client = NatsClient::connect(config.with_reconnect(quick_reconnects()))
        .await
        .expect("connects");
    client.flush().await.expect("answers");

    proxy.cut();
    log.wait_for("the secret could not be read again", BUDGET)
        .await;
    let mut events = client.subscribe_events();
    client.force_reconnect().await.expect("reconnect requested");
    next_event(&mut events, "the reconnect", |event| {
        event.state == ConnectionState::Connected
    })
    .await;
    client
        .flush()
        .await
        .expect("reconnected on the cached password while the store is down");

    let text = log.text();
    let warning = text
        .lines()
        .find(|line| line.contains("the secret could not be read again"))
        .expect("the failed refresh is logged");
    assert!(
        warning.contains("error_code=\"SECRETS_003\"")
            || warning.contains("error_code=SECRETS_003"),
        "{warning}"
    );
    assert!(warning.contains("stale_seconds="), "{warning}");
    assert!(
        warning.contains(&path),
        "the warning names the path: {warning}"
    );
    assert!(!text.contains(password.expose()), "the password was logged");
}
