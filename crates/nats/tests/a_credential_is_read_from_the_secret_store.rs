//! A NATS password or token declared by reference is read from the secret store at connect
//! time, reaches a server that refuses every other credential, and appears nowhere else.

mod support;

use std::fs;
use std::net::SocketAddr;

use fundamental_config::AppSettings;
use fundamental_core::Environment;
use fundamental_core::Error as _;
use fundamental_nats::{NatsClient, NatsClientAuth, NatsConfig, NatsError, ReconnectConfig};
use fundamental_test_containers::TcpProxy;
use fundamental_test_containers::nats::{NatsAuthServer, NatsTokenServer, NatsUser};
use serde_json::json;
use support::{CapturedLog, nats_settings, openbao, unique, vault_at};

/// A reference to `path`'s `key` in the development server's `secret/` mount.
fn reference(path: &str, key: &str) -> String {
    format!("{{ source = \"vault_kv\", mount = \"secret\", path = \"{path}\", key = \"{key}\" }}")
}

#[tokio::test]
async fn a_referenced_password_reaches_an_auth_on_server() {
    let bao = openbao().await;
    let path = unique("app-password");
    bao.kv_put(
        &path,
        &json!({ "password": NatsUser::App.password().expose(), "other": "not-read" }),
    )
    .await
    .expect("the password is stored");
    let server = NatsAuthServer::shared().await.expect("the auth server");
    let settings = nats_settings(&format!(
        "url = \"{}\"\nusername = \"{}\"\npassword = {}\n",
        server.url(),
        NatsUser::App.username(),
        reference(&path, "password")
    ));

    let vault = vault_at(bao.address()).await;
    let config = NatsConfig::from_settings(&settings, Environment::Development, Some(&vault))
        .await
        .expect("the reference is read");
    assert!(matches!(
        config.auth,
        NatsClientAuth::StoredUserPassword { .. }
    ));
    let client = NatsClient::connect(config.with_reconnect(ReconnectConfig::fail_fast()))
        .await
        .expect("the referenced password lets the app user in");
    client.flush().await.expect("and the server answers it");
}

#[tokio::test]
async fn a_referenced_token_reaches_a_token_server() {
    let bao = openbao().await;
    let path = unique("app-token");
    bao.kv_put(
        &path,
        &json!({ "token": NatsTokenServer::token().expose() }),
    )
    .await
    .expect("the token is stored");
    let server = NatsTokenServer::shared().await.expect("the token server");
    let settings = nats_settings(&format!(
        "url = \"{}\"\ntoken = {}\n",
        server.url(),
        reference(&path, "token")
    ));

    let vault = vault_at(bao.address()).await;
    let config = NatsConfig::from_settings(&settings, Environment::Development, Some(&vault))
        .await
        .expect("the reference is read");
    assert!(matches!(config.auth, NatsClientAuth::StoredToken { .. }));
    let client = NatsClient::connect(config.with_reconnect(ReconnectConfig::fail_fast()))
        .await
        .expect("the referenced token lets the client in");
    client.flush().await.expect("and the server answers it");
}

#[tokio::test]
async fn an_unreadable_reference_refuses_boot_with_its_path() {
    let bao = openbao().await;
    let vault = vault_at(bao.address()).await;
    let stored = unique("no-such-key");
    bao.kv_put(&stored, &json!({ "username": "app" }))
        .await
        .expect("a secret without the key");
    let missing = unique("never-written");

    for (path, key, cause) in [
        (missing.as_str(), "password", "SECRETS_004"),
        (stored.as_str(), "password", "SECRETS_008"),
    ] {
        let settings = nats_settings(&format!(
            "username = \"app\"\npassword = {}\n",
            reference(path, key)
        ));
        let refused = NatsConfig::from_settings(&settings, Environment::Development, Some(&vault))
            .await
            .expect_err("an unreadable reference refuses boot");
        let NatsError::SecretUnreadable {
            cause: reported, ..
        } = &refused
        else {
            panic!("expected NATS_023, got {refused:?}");
        };
        assert_eq!(refused.error_code(), "NATS_023");
        assert_eq!(reported.error_code(), cause, "{refused}");
        assert!(
            !refused.is_retryable(),
            "the store answered; asking again changes nothing"
        );
        assert!(
            refused
                .to_string()
                .contains(&format!("vault_kv secret/{path}#{key}")),
            "the error names where the secret is: {refused}"
        );
    }

    let upstream: SocketAddr = bao
        .address()
        .trim_start_matches("http://")
        .parse()
        .expect("a loopback address");
    let proxy = TcpProxy::start(upstream).await.expect("a proxy");
    let through = vault_at(&format!("http://{}", proxy.address())).await;
    proxy.cut();
    let settings = nats_settings(&format!(
        "username = \"app\"\npassword = {}\n",
        reference(&stored, "username")
    ));
    let refused = NatsConfig::from_settings(&settings, Environment::Development, Some(&through))
        .await
        .expect_err("an unreachable store refuses boot");
    assert_eq!(refused.error_code(), "NATS_023");
    assert!(
        refused.is_retryable(),
        "an unreachable store may answer later: {refused:?}"
    );
}

#[tokio::test]
async fn a_field_that_is_not_a_string_is_refused_without_its_value() {
    let (log, _guard) = CapturedLog::install();
    let bao = openbao().await;
    let path = unique("numeric");
    bao.kv_put(&path, &json!({ "token": 987_654_321 }))
        .await
        .expect("stored");
    let vault = vault_at(bao.address()).await;
    let settings = nats_settings(&format!("token = {}\n", reference(&path, "token")));
    let refused = NatsConfig::from_settings(&settings, Environment::Development, Some(&vault))
        .await
        .expect_err("a token is a string");
    let NatsError::SecretUnreadable { cause, .. } = &refused else {
        panic!("expected NATS_023, got {refused:?}");
    };
    assert_eq!(cause.error_code(), "SECRETS_005");
    let everything = format!("{refused}\n{refused:?}\n{cause}\n{}", log.text());
    assert!(
        !everything.contains("987654321"),
        "the value reached an error or a log"
    );
}

#[tokio::test]
async fn an_answer_larger_than_the_limit_is_refused_unread() {
    let bao = openbao().await;
    let path = unique("oversized");
    let filler = "x".repeat(fundamental_secrets::KV_BODY_LIMIT);
    bao.kv_put(&path, &json!({ "password": "pw", "filler": filler }))
        .await
        .expect("stored");
    let vault = vault_at(bao.address()).await;
    let settings = nats_settings(&format!(
        "username = \"app\"\npassword = {}\n",
        reference(&path, "password")
    ));
    let refused = NatsConfig::from_settings(&settings, Environment::Development, Some(&vault))
        .await
        .expect_err("a credential is never that large");
    let NatsError::SecretUnreadable { cause, .. } = &refused else {
        panic!("expected NATS_023, got {refused:?}");
    };
    assert_eq!(cause.error_code(), "SECRETS_005");
    assert!(cause.to_string().contains("exceeds 65536 bytes"), "{cause}");
    assert!(!refused.is_retryable());
}

#[tokio::test]
async fn a_referenced_password_is_never_logged() {
    let (log, _guard) = CapturedLog::install();
    let bao = openbao().await;
    let path = unique("never-logged");
    let password = NatsUser::App.password();
    bao.kv_put(&path, &json!({ "password": password.expose() }))
        .await
        .expect("the password is stored");
    let server = NatsAuthServer::shared().await.expect("the auth server");
    let settings = nats_settings(&format!(
        "url = \"{}\"\nusername = \"app\"\npassword = {}\n",
        server.url(),
        reference(&path, "password")
    ));
    let vault = vault_at(bao.address()).await;
    let config = NatsConfig::from_settings(&settings, Environment::Development, Some(&vault))
        .await
        .expect("the reference is read");
    let client = NatsClient::connect(config.clone().with_reconnect(ReconnectConfig::fail_fast()))
        .await
        .expect("connects");
    client.flush().await.expect("answers");

    let wrong = nats_settings(&format!(
        "username = \"app\"\npassword = {}\n",
        reference(&path, "absent")
    ));
    let refused = NatsConfig::from_settings(&wrong, Environment::Development, Some(&vault))
        .await
        .expect_err("refused");

    let everything = format!(
        "{}\n{config:?}\n{settings:?}\n{refused}\n{refused:?}",
        log.text()
    );
    assert!(
        log.text()
            .contains("NATS credential read from the secret store"),
        "the read is logged: {}",
        log.text()
    );
    let leaks: Vec<&str> = everything
        .lines()
        .filter(|line| line.contains(password.expose()))
        .collect();
    assert!(
        leaks.is_empty(),
        "the password reached a log, a Debug or an error: {leaks:#?}"
    );
}

#[tokio::test]
async fn connections_built_from_one_reference_share_one_refresher() {
    let (log, _guard) = CapturedLog::install();
    let bao = openbao().await;
    let path = unique("shared");
    bao.kv_put(&path, &json!({ "password": "pw" }))
        .await
        .expect("stored");
    let settings = nats_settings(&format!(
        "username = \"app\"\npassword = {}\n",
        reference(&path, "password")
    ));
    let vault = vault_at(bao.address()).await;
    let mut configs = Vec::new();
    for _ in 0..4 {
        configs.push(
            NatsConfig::from_settings(&settings, Environment::Development, Some(&vault))
                .await
                .expect("resolves"),
        );
    }
    assert_eq!(
        log.count("secret read; refreshing it in the background"),
        1,
        "one read and one refresher for one reference: {}",
        log.text()
    );
    assert_eq!(
        log.count("secret already refreshed for this client; shared"),
        3
    );
}

#[tokio::test]
async fn a_live_credential_cannot_be_serialised_as_no_credential() {
    let bao = openbao().await;
    let path = unique("serialised");
    bao.kv_put(&path, &json!({ "password": "pw" }))
        .await
        .expect("stored");
    let settings = nats_settings(&format!(
        "username = \"app\"\npassword = {}\n",
        reference(&path, "password")
    ));
    let vault = vault_at(bao.address()).await;
    let config = NatsConfig::from_settings(&settings, Environment::Development, Some(&vault))
        .await
        .expect("resolves");
    assert!(
        serde_json::to_string(&config).is_err(),
        "written out and read back, it would connect with no credential"
    );
}

/// An app's NATS password read from the secret store through one configuration line.
///
/// The `[apps.shop.messaging.nats]` and `[apps.shop.vault]` sections an app writes, with the
/// password a reference to an OpenBao mount, path and key (`acme`, `nats/shop`, `password`). The
/// `url` and `username` are the auth test server's, and OpenBao is reached with the development
/// root token.
#[tokio::test]
async fn an_app_configuration_with_a_secret_reference_loads_and_connects() {
    let bao = openbao().await;
    bao.kv_put_at(
        "acme",
        "nats/shop",
        &json!({ "password": NatsUser::App.password().expose() }),
    )
    .await
    .expect("shop's password is stored in the secret store");
    let server = NatsAuthServer::shared().await.expect("the auth server");

    let project = tempfile::tempdir().expect("a project directory");
    let manifest = project.path().join("fun.toml");
    let toml = format!(
        r#"
[apps.shop.messaging.nats]
url = "{url}"
username = "{user}"
connection_name = "shop"
password = {{ source = "vault_kv", mount = "acme", path = "nats/shop", key = "password" }}

[apps.shop.vault]
enabled = true
address = "{address}"
auth = {{ method = "token", token = "fundamental-test-root" }}
"#,
        url = server.url(),
        user = NatsUser::App.username(),
        address = bao.address(),
    );
    fs::write(&manifest, toml).expect("written");

    let app = AppSettings::load_in("shop", &manifest, Environment::Development)
        .expect("shop's configuration loads");
    let vault = fundamental_secrets::VaultClient::from_settings(&app.config().secrets)
        .await
        .expect("shop logs in to OpenBao")
        .expect("its vault is enabled");
    let config = NatsConfig::from_settings(
        app.nats_settings().expect("shop declares NATS"),
        app.environment(),
        Some(&vault),
    )
    .await
    .expect("shop's password is read from acme/nats/shop");
    assert_eq!(config.name.as_deref(), Some("shop"));
    let client = NatsClient::connect(config.with_reconnect(ReconnectConfig::fail_fast()))
        .await
        .expect("shop connects with the password OpenBao holds");
    client.flush().await.expect("and the server answers it");
}
