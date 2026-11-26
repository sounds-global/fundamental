//! A NATS NKey whose seed is a non-exportable Transit key: the server holds only the public
//! key, the process holds no secret, and each connection attempt has Transit sign the nonce.

mod support;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use fundamental_core::Environment;
use fundamental_core::Error as _;
use fundamental_nats::{
    ConnectionState, NatsClient, NatsClientAuth, NatsConfig, NatsError, NkeyTransitSigner,
    ReconnectConfig, nkey_user_public_key,
};
use fundamental_secrets::VaultClient;
use fundamental_secrets::transit::{
    TransitKeyName, TransitKeyType, TransitMount, ed25519_public_key_bytes,
};
use fundamental_test_containers::TcpProxy;
use fundamental_test_containers::nats::NatsNkeyServer;
use serde_json::json;
use support::{CapturedLog, TRANSIT, eventually, nats_settings, openbao, vault_at};

/// The Transit key the NKey server trusts, fixed so the server's configuration stays the same.
const KEY: &str = "nats-nkey-app";

const BUDGET: Duration = Duration::from_secs(20);

/// The user NKey version 1 of Transit key `name` signs as, creating the key if it is absent.
async fn public_key_of(vault: &VaultClient, name: &str) -> String {
    let mount = TransitMount::parse(TRANSIT).expect("mount");
    let name = TransitKeyName::parse(name).expect("name");
    let info = vault
        .transit_ensure_signing_key(&mount, &name, TransitKeyType::Ed25519)
        .await
        .expect("the key exists");
    let raw = ed25519_public_key_bytes(&info.public_keys[&1]).expect("an ed25519 key");
    nkey_user_public_key(&raw)
}

/// `[messaging.nats]` signing as `public_key` with Transit key `key`.
fn nkey_settings(
    url: &str,
    public_key: &str,
    key: &str,
) -> fundamental_config::settings::NatsSettings {
    nats_settings(&format!(
        "url = \"{url}\"\nnkey = {{ public_key = \"{public_key}\", signer = {{ source = \"vault_transit\", mount = \"{TRANSIT}\", key = \"{key}\" }} }}\n"
    ))
}

#[tokio::test]
async fn a_transit_signed_nkey_authenticates_against_nats_server() {
    let bao = openbao().await;
    let vault = vault_at(bao.address()).await;
    let public_key = public_key_of(&vault, KEY).await;
    let server = NatsNkeyServer::shared(&public_key)
        .await
        .expect("the NKey server");

    let config = NatsConfig::from_settings(
        &nkey_settings(server.url(), &public_key, KEY),
        Environment::Development,
        Some(&vault),
    )
    .await
    .expect("the Transit key is this NKey");
    assert!(matches!(config.auth, NatsClientAuth::NKey(_)));
    let client = NatsClient::connect(config.with_reconnect(ReconnectConfig::fail_fast()))
        .await
        .expect("the server lets in the holder of the seed, which only Transit is");
    client.flush().await.expect("and answers it");

    let unsigned = NatsClient::connect(server.anonymous_config())
        .await
        .expect_err("nobody else is let in");
    assert_eq!(unsigned.error_code(), "NATS_003");
}

#[tokio::test]
async fn a_rotation_in_transit_does_not_change_which_key_signs() {
    let bao = openbao().await;
    let vault = vault_at(bao.address()).await;
    let public_key = public_key_of(&vault, KEY).await;
    bao.post(&format!("{TRANSIT}/keys/{KEY}/rotate"), &json!({}))
        .await
        .expect("rotated");
    let server = NatsNkeyServer::shared(&public_key)
        .await
        .expect("the NKey server");
    let client = NatsClient::connect(
        NatsConfig::from_settings(
            &nkey_settings(server.url(), &public_key, KEY),
            Environment::Development,
            Some(&vault),
        )
        .await
        .expect("the configured public key selects version 1")
        .with_reconnect(ReconnectConfig::fail_fast()),
    )
    .await
    .expect("still signs with the version the configuration names");
    client.flush().await.expect("answers");
}

#[tokio::test]
async fn a_transit_key_whose_public_key_differs_refuses_boot() {
    let bao = openbao().await;
    let vault = vault_at(bao.address()).await;
    let actual = public_key_of(&vault, KEY).await;
    let someone_else = nkeys::KeyPair::new_user().public_key();

    let refused = NatsConfig::from_settings(
        &nkey_settings("nats://127.0.0.1:1", &someone_else, KEY),
        Environment::Development,
        Some(&vault),
    )
    .await
    .expect_err("the key is not this NKey");
    assert!(
        matches!(refused, NatsError::NkeyUnusable { .. }),
        "{refused:?}"
    );
    assert_eq!(refused.error_code(), "NATS_024");
    assert!(!refused.is_retryable());
    assert!(
        refused.to_string().contains(&format!("v1 {actual}")),
        "the refusal names what the key does sign as: {refused}"
    );

    bao.post(
        &format!("{TRANSIT}/keys/nats-nkey-exportable"),
        &json!({ "type": "ed25519", "exportable": true }),
    )
    .await
    .expect("an exportable key");
    bao.post(
        &format!("{TRANSIT}/keys/nats-nkey-p256"),
        &json!({ "type": "ecdsa-p256" }),
    )
    .await
    .expect("a P-256 key");
    for (key, why) in [
        ("nats-nkey-exportable", "may leave the secret store"),
        ("nats-nkey-p256", "an NKey is an ed25519 key"),
    ] {
        let refused = NatsConfig::from_settings(
            &nkey_settings("nats://127.0.0.1:1", &someone_else, key),
            Environment::Development,
            Some(&vault),
        )
        .await
        .expect_err(key);
        assert_eq!(refused.error_code(), "NATS_024", "{key}");
        assert!(refused.to_string().contains(why), "{key}: {refused}");
    }

    let refused = NatsConfig::from_settings(
        &nkey_settings("nats://127.0.0.1:1", &actual, KEY),
        Environment::Development,
        None,
    )
    .await
    .expect_err("no store to sign with");
    assert_eq!(refused.error_code(), "NATS_024");
}

#[tokio::test]
async fn a_reconnect_with_transit_unreachable_fails_coded_and_recovers() {
    let (log, _guard) = CapturedLog::install();
    let bao = openbao().await;
    let direct = vault_at(bao.address()).await;
    let public_key = public_key_of(&direct, KEY).await;
    let upstream: SocketAddr = bao
        .address()
        .trim_start_matches("http://")
        .parse()
        .expect("a loopback address");
    let proxy = TcpProxy::start(upstream).await.expect("a proxy");
    let vault = vault_at(&format!("http://{}", proxy.address())).await;
    let server = NatsNkeyServer::shared(&public_key)
        .await
        .expect("the NKey server");
    let client = NatsClient::connect(
        NatsConfig::from_settings(
            &nkey_settings(server.url(), &public_key, KEY),
            Environment::Development,
            Some(&vault),
        )
        .await
        .expect("resolves")
        .with_reconnect(
            ReconnectConfig::default()
                .with_initial_delay(Duration::from_millis(100))
                .with_max_delay(Duration::from_millis(500)),
        ),
    )
    .await
    .expect("connects");
    client.flush().await.expect("answers");

    proxy.cut();
    client.force_reconnect().await.expect("reconnect requested");
    log.wait_for("NATS_025", BUDGET).await;
    eventually("the connection to drop without a signature", BUDGET, || {
        !client.is_connected()
    })
    .await;

    proxy.restore();
    eventually("the reconnect once Transit is back", BUDGET, || {
        client.is_connected()
    })
    .await;
    client.flush().await.expect("answers again");
    assert_eq!(client.state(), ConnectionState::Connected);
    let failure = log
        .text()
        .lines()
        .find(|line| line.contains("NATS_025"))
        .map(str::to_owned)
        .expect("the failed attempt is logged");
    assert!(failure.contains("retryable=true"), "{failure}");
}

/// A Transit engine that publishes one key and signs with another.
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn transit_signing_with_the_wrong_key(published: &[u8; 32]) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let key_read = json!({ "data": {
        "type": "ed25519", "exportable": false, "allow_plaintext_backup": false,
        "latest_version": 1,
        "keys": { "1": { "public_key": data_encoding::BASE64.encode(published) } },
    }})
    .to_string();
    let impostor = nkeys::KeyPair::new_user();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let key_read = key_read.clone();
            let impostor =
                nkeys::KeyPair::from_seed(&impostor.seed().expect("seed")).expect("pair");
            tokio::spawn(async move {
                let mut pending = Vec::new();
                let mut chunk = [0_u8; 4096];
                loop {
                    let Some(end) = pending.windows(4).position(|w| w == b"\r\n\r\n") else {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => pending.extend_from_slice(&chunk[..read]),
                        }
                        continue;
                    };
                    let head = String::from_utf8_lossy(&pending[..end]).to_string();
                    let length: usize = head
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    while pending.len() < end + 4 + length {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => pending.extend_from_slice(&chunk[..read]),
                        }
                    }
                    let body: serde_json::Value =
                        serde_json::from_slice(&pending[end + 4..end + 4 + length])
                            .unwrap_or_default();
                    pending.drain(..end + 4 + length);
                    let answer = if head.starts_with("POST") && head.contains("/sign/") {
                        let input = body["input"].as_str().unwrap_or_default();
                        let nonce = data_encoding::BASE64
                            .decode(input.as_bytes())
                            .unwrap_or_default();
                        let signature = impostor.sign(&nonce).expect("signs");
                        json!({ "data": { "signature": format!("vault:v1:{}", data_encoding::BASE64.encode(&signature)) } }).to_string()
                    } else {
                        key_read.clone()
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{answer}",
                        answer.len()
                    );
                    if socket.write_all(response.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    format!("http://{address}")
}

#[tokio::test]
async fn a_signature_that_does_not_verify_is_refused_before_it_is_sent() {
    let configured = nkeys::KeyPair::new_user();
    let (_, raw) = nkeys::from_public_key(&configured.public_key()).expect("decodes");
    let transit = transit_signing_with_the_wrong_key(&raw).await;
    let vault = vault_at(&transit).await;
    let signer = NkeyTransitSigner::connect(
        Some(&vault),
        &fundamental_config::settings::NkeySigner::VaultTransit {
            mount: TRANSIT.to_owned(),
            key: "impostor".to_owned(),
        },
        &configured.public_key().try_into().expect("a user key"),
    )
    .await
    .expect("the key it publishes is the configured one");

    let refused = signer
        .sign(b"KIXx6ZpVJkLFJ0M")
        .await
        .expect_err("a signature from another key is never sent");
    assert!(
        matches!(refused, NatsError::NkeyUnusable { .. }),
        "{refused:?}"
    );
    assert_eq!(refused.error_code(), "NATS_024");
    assert!(
        refused
            .to_string()
            .contains("did not verify against the public key"),
        "{refused}"
    );
}

#[tokio::test]
async fn a_payload_that_is_not_a_nats_nonce_is_refused_unsigned() {
    let bao = openbao().await;
    let vault = vault_at(bao.address()).await;
    let public_key = public_key_of(&vault, KEY).await;
    let signer = NkeyTransitSigner::connect(
        Some(&vault),
        &fundamental_config::settings::NkeySigner::VaultTransit {
            mount: TRANSIT.to_owned(),
            key: KEY.to_owned(),
        },
        &public_key.try_into().expect("a user key"),
    )
    .await
    .expect("the signer");

    for hostile in [
        &b"eyJhbGciOiJFZERTQSJ9.eyJzdWIiOiJhZG1pbiJ9"[..],
        b"",
        &[b'a'; 65][..],
        b"nonce with spaces",
    ] {
        let refused = signer
            .sign(hostile)
            .await
            .expect_err("only a NATS nonce is signed");
        assert_eq!(refused.error_code(), "NATS_025");
        assert!(!refused.is_retryable());
        assert!(
            refused.to_string().contains("not a NATS nonce"),
            "{refused}"
        );
    }
    signer
        .sign(b"KIXx6ZpVJkLFJ0M")
        .await
        .expect("a nonce of the shape nats-server sends is signed");
}

/// What one nonce signature costs against the loopback OpenBao, a lower bound.
#[tokio::test]
async fn a_transit_nonce_signature_is_measured() {
    const SAMPLES: usize = 200;
    let bao = openbao().await;
    let vault = vault_at(bao.address()).await;
    let public_key = public_key_of(&vault, KEY).await;
    let signer = NkeyTransitSigner::connect(
        Some(&vault),
        &fundamental_config::settings::NkeySigner::VaultTransit {
            mount: TRANSIT.to_owned(),
            key: KEY.to_owned(),
        },
        &public_key.clone().try_into().expect("a user key"),
    )
    .await
    .expect("the signer");

    let mut took = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let nonce = format!("nonce-{sample}");
        let started = Instant::now();
        signer
            .sign(nonce.as_bytes())
            .await
            .expect("signed and verified");
        took.push(started.elapsed());
    }
    took.sort_unstable();
    let percentile = |p: usize| took[(SAMPLES * p / 100).min(SAMPLES - 1)];
    eprintln!(
        "transit ed25519 nonce signature, {SAMPLES} samples, loopback OpenBao: p50 {:?}, p99 {:?}, max {:?}",
        percentile(50),
        percentile(99),
        took[SAMPLES - 1]
    );
    assert!(
        percentile(99) < Duration::from_secs(3),
        "a signature must fit inside the signer's deadline"
    );
}
