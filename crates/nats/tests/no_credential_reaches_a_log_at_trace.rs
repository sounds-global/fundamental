//! With the framework's own subscriber at `trace`, a connection and a reconnect on a password
//! held in the secret store write the password nowhere.
//!
//! The driver logs each `CONNECT` with its credential at `trace`; the builder caps that target.
//! Its own binary, because it installs the global subscriber.

mod support;

use std::fs;
use std::time::Duration;

use fundamental_config::AppSettings;
use fundamental_core::Environment;
use fundamental_nats::{ConnectionState, NatsClient, NatsConfig, ReconnectConfig};
use fundamental_observability::ObservabilityBuilder;
use fundamental_observability::testing::CapturedLogs;
use fundamental_test_containers::nats::{NatsAuthServer, NatsUser};
use serde_json::json;
use support::{eventually, openbao, unique};

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn no_credential_reaches_a_log_at_trace() {
    // Installed first: the first subscriber in a process is the only one.
    let captured = CapturedLogs::default();
    let _telemetry = ObservabilityBuilder::from_app(&load(
        "[apps.orders.observability]\nlog_level = \"trace\"\nspan_level = \"trace\"\n",
    ))
    .with_layer(captured.layer())
    // A span directive is consulted before every static one.
    .with_directives(["[connect]=trace"])
    .init()
    .expect("the framework's subscriber installs");

    let bao = openbao().await;
    let path = unique("at-trace");
    let password = NatsUser::App.password();
    bao.kv_put(&path, &json!({ "password": password.expose() }))
        .await
        .expect("the password is stored");
    let server = NatsAuthServer::shared().await.expect("the auth server");

    let app = load(&format!(
        r#"
[apps.orders.messaging.nats]
url = "{url}"
username = "{user}"
password = {{ source = "vault_kv", mount = "secret", path = "{path}", key = "password" }}

[apps.orders.vault]
enabled = true
address = "{address}"
auth = {{ method = "token", token = "fundamental-test-root" }}
"#,
        url = server.url(),
        user = NatsUser::App.username(),
        address = bao.address(),
    ));

    let vault = fundamental_secrets::VaultClient::from_settings(&app.config().secrets)
        .await
        .expect("logs in")
        .expect("enabled");
    let client = NatsClient::connect(
        NatsConfig::from_settings(
            app.nats_settings().expect("declared"),
            app.environment(),
            Some(&vault),
        )
        .await
        .expect("the password is read")
        .with_reconnect(ReconnectConfig::default().with_initial_delay(Duration::from_millis(100))),
    )
    .await
    .expect("connects");
    client.flush().await.expect("answers");
    let mut events = client.subscribe_events();
    client.force_reconnect().await.expect("reconnect requested");
    let reconnected = tokio::time::timeout(Duration::from_secs(20), async {
        while let Ok(event) = events.recv().await {
            if event.state == ConnectionState::Connected {
                return;
            }
        }
    })
    .await;
    assert!(reconnected.is_ok(), "the client reconnected");
    eventually("the reconnect", Duration::from_secs(20), || {
        client.is_connected()
    })
    .await;
    client.flush().await.expect("answers after the reconnect");

    let inline = NatsClient::connect(
        NatsConfig::from_settings(
            app.nats_settings().expect("declared"),
            app.environment(),
            Some(&vault),
        )
        .await
        .expect("the password is read")
        .with_reconnect(ReconnectConfig::fail_fast()),
    )
    .await
    .expect("connects inline");
    inline.flush().await.expect("answers");

    let text = captured.text();
    assert!(
        text.contains("TRACE"),
        "the subscriber ran at trace, so the absence below means something"
    );
    let leaks: Vec<&str> = text
        .lines()
        .filter(|line| line.contains(password.expose()))
        .collect();
    assert!(leaks.is_empty(), "the password reached the log: {leaks:#?}");
}

/// AppSettings `orders` from a manifest holding `toml`.
fn load(toml: &str) -> AppSettings {
    let project = tempfile::tempdir().expect("a project directory");
    let manifest = project.path().join("fun.toml");
    fs::write(&manifest, toml).expect("written");
    AppSettings::load_in("orders", &manifest, Environment::Development)
        .expect("the configuration loads")
}
