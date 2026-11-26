//! Signing through a PKI role, against a real OpenBao engine.
//!
//! Proves the role, not the caller, decides what a certificate may be. Needs Docker, and
//! fails, never skips, without it.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fundamental_config::settings::{VaultAuth, VaultSettings};
use fundamental_core::secrets::InMemorySecret;
use fundamental_secrets::pki::{PkiMount, PkiRole};
use fundamental_secrets::{SecretError, VaultClient};
use fundamental_test_containers::container_name::OPENBAO_05_PKI;
use fundamental_test_containers::openbao::OpenBaoServer;
use serde_json::{Value, json};

const ROOT: &str = "pki-suite-root";
const REQUESTS: &str = "pki-suite-requests";
const ROLE: &str = "operator-device";
const SEVEN_DAYS: u64 = 7 * 24 * 3600;

async fn arranged(server: &OpenBaoServer, path: &str, body: Value) -> Value {
    let response = server.post(path, &body).await.expect("the request is sent");
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    assert!(
        status.is_success() || text.contains("path is already in use"),
        "arranging {path}: {status} {text}"
    );
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

/// A root, a seven-day client-only role under it, and a second mount that makes real CSRs.
async fn engine() -> (&'static OpenBaoServer, VaultClient) {
    let server = OpenBaoServer::shared(OPENBAO_05_PKI)
        .await
        .expect("an OpenBao container (is Docker running?)");
    for mount in [ROOT, REQUESTS] {
        arranged(
            server,
            &format!("sys/mounts/{mount}"),
            json!({ "type": "pki", "config": { "max_lease_ttl": "87600h" } }),
        )
        .await;
    }
    arranged(
        server,
        &format!("{ROOT}/root/generate/internal"),
        json!({ "common_name": "suite operator root", "ttl": "8760h" }),
    )
    .await;
    arranged(
        server,
        &format!("{ROOT}/roles/{ROLE}"),
        json!({
            "max_ttl": "168h",
            "not_before_duration": "0s",
            "client_flag": true,
            "server_flag": false,
            "code_signing_flag": false,
            "email_protection_flag": false,
            "key_usage": ["DigitalSignature"],
            "ext_key_usage": [],
            "use_csr_common_name": false,
            "use_csr_sans": false,
            "require_cn": false,
            "allow_ip_sans": false,
            "allowed_uri_sans": "",
            "allowed_other_sans": "",
            "key_type": "ec",
            "key_bits": 256,
        }),
    )
    .await;
    let settings = VaultSettings {
        enabled: true,
        address: server.address().to_owned(),
        auth: VaultAuth::Token {
            token: InMemorySecret::new(server.root_token().to_owned()),
        },
        ..VaultSettings::default()
    };
    let client = VaultClient::from_settings(&settings)
        .await
        .expect("client")
        .expect("enabled");
    (server, client)
}

/// A real P-256 certificate request, made by the engine itself.
async fn csr(server: &OpenBaoServer) -> String {
    let answer = arranged(
        server,
        &format!("{REQUESTS}/intermediate/generate/internal"),
        json!({ "common_name": "a device", "key_type": "ec", "key_bits": 256 }),
    )
    .await;
    answer
        .pointer("/data/csr")
        .and_then(Value::as_str)
        .expect("the engine returned a request")
        .to_owned()
}

fn unix_now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("after the epoch")
            .as_secs(),
    )
    .expect("fits")
}

fn names() -> (PkiMount, PkiRole) {
    (
        PkiMount::parse(ROOT).expect("valid"),
        PkiRole::parse(ROLE).expect("valid"),
    )
}

#[tokio::test]
async fn a_request_is_signed_through_its_role() {
    let (server, vault) = engine().await;
    let (mount, role) = names();
    let issued = vault
        .pki_sign(&mount, &role, &csr(server).await, Duration::from_secs(3600))
        .await
        .expect("signed");
    assert!(
        issued
            .certificate_pem
            .starts_with("-----BEGIN CERTIFICATE-----")
    );
    assert!(
        issued
            .issuing_ca_pem
            .starts_with("-----BEGIN CERTIFICATE-----")
    );
    assert!(!issued.serial_number.is_empty());
    let remaining = issued.expiration - unix_now();
    assert!((3500..=3700).contains(&remaining), "{remaining}s");
}

#[tokio::test]
async fn custody_never_issues_longer_than_its_role() {
    let (server, vault) = engine().await;
    let (mount, role) = names();
    let issued = vault
        .pki_sign(
            &mount,
            &role,
            &csr(server).await,
            Duration::from_secs(8 * 24 * 3600),
        )
        .await
        .expect("custody answers with what its role allows");
    let lifetime = issued.expiration - unix_now();
    assert!(
        lifetime <= i64::try_from(SEVEN_DAYS).expect("fits") + 60,
        "asked for eight days and received {lifetime}s"
    );
}

#[tokio::test]
async fn something_that_is_not_a_request_never_reaches_custody() {
    let (_, vault) = engine().await;
    let (mount, role) = names();
    let refused = vault
        .pki_sign(
            &mount,
            &role,
            "-----BEGIN CERTIFICATE-----\nx\n-----END CERTIFICATE-----",
            Duration::from_secs(60),
        )
        .await
        .expect_err("a certificate is not a request");
    assert!(
        matches!(refused, SecretError::InvalidConfig(_)),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_role_that_does_not_exist_is_refused_by_custody() {
    let (server, vault) = engine().await;
    let mount = PkiMount::parse(ROOT).expect("valid");
    let absent = PkiRole::parse("no-such-role").expect("valid");
    assert!(
        vault
            .pki_sign(&mount, &absent, &csr(server).await, Duration::from_secs(60))
            .await
            .is_err()
    );
}

/// The `content-length` a request head declares, zero when it declares none.
fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|value| value.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(0)
}

/// Answers one request the way `auth/cert/login` does, and reports what it was asked.
async fn one_login_answer() -> (String, tokio::task::JoinHandle<(String, Value)>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = format!("http://{}", listener.local_addr().expect("bound"));
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("the client connects");
        let mut received = Vec::new();
        let mut chunk = [0_u8; 4096];
        let (head, body) = loop {
            let read = stream.read(&mut chunk).await.expect("reads");
            received.extend_from_slice(&chunk[..read]);
            let text = String::from_utf8_lossy(&received).into_owned();
            if let Some((head, body)) = text.split_once("\r\n\r\n")
                && body.len() >= content_length(head)
            {
                break (head.to_owned(), body.to_owned());
            }
        };
        let reply =
            r#"{"auth":{"client_token":"cert-login-token","lease_duration":0,"renewable":false}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
            reply.len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("answers");
        let request_line = head.lines().next().unwrap_or_default().to_owned();
        (
            request_line,
            serde_json::from_str(&body).unwrap_or(Value::Null),
        )
    });
    (address, task)
}

#[tokio::test]
async fn cert_login_presents_its_identity_and_names_its_role() {
    let (server, _) = engine().await;
    let issued = arranged(server, &format!("{ROOT}/issue/{ROLE}"), json!({})).await;
    let pem = |field: &str| {
        issued
            .pointer(&format!("/data/{field}"))
            .and_then(Value::as_str)
            .expect("the engine issued a key pair")
            .to_owned()
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let cert_path = dir.path().join("client.crt");
    let key_path = dir.path().join("client.key");
    std::fs::write(&cert_path, pem("certificate")).expect("write");
    std::fs::write(&key_path, pem("private_key")).expect("write");

    let (address, answered) = one_login_answer().await;
    let settings = VaultSettings {
        enabled: true,
        address,
        auth: VaultAuth::Cert {
            role: Some("ops-ca".to_owned()),
            client_cert_path: cert_path.to_string_lossy().into_owned(),
            client_key_path: key_path.to_string_lossy().into_owned(),
        },
        ..VaultSettings::default()
    };
    VaultClient::from_settings(&settings)
        .await
        .expect("logged in")
        .expect("enabled");

    let (request_line, body) = answered.await.expect("the stub answered");
    assert!(
        request_line.starts_with("POST /v1/auth/cert/login "),
        "{request_line}"
    );
    assert_eq!(body, json!({ "name": "ops-ca" }));
}

#[tokio::test]
async fn cert_login_refuses_a_key_file_that_is_not_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cert_path = dir.path().join("client.crt");
    let key_path = dir.path().join("client.key");
    std::fs::write(&cert_path, "not a certificate").expect("write");
    std::fs::write(&key_path, "not a key").expect("write");
    let settings = VaultSettings {
        enabled: true,
        address: "http://127.0.0.1:9".to_owned(),
        auth: VaultAuth::Cert {
            role: None,
            client_cert_path: cert_path.to_string_lossy().into_owned(),
            client_key_path: key_path.to_string_lossy().into_owned(),
        },
        ..VaultSettings::default()
    };
    let refused = VaultClient::from_settings(&settings)
        .await
        .expect_err("no identity can be built");
    assert!(
        matches!(refused, SecretError::InvalidConfig(_)),
        "{refused:?}"
    );
}
