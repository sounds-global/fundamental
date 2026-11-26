//! A token the verifier refuses leaves nothing of itself in the logs.
//!
//! One test in its own binary: it installs the process's global subscriber.

use std::net::SocketAddr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fundamental_auth::{
    AuthError, InvalidToken, JwtVerifier, MultiIssuerVerifier, PrincipalVerifier, TrustedIssuer,
    UntrustedToken,
};
use fundamental_config::settings::AuthSettings;
use fundamental_core::secrets::SecretString;
use fundamental_observability::testing::CapturedLogs;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, encode};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

mod support;

use support::Es256;

const EMAIL: &str = "jan@example.nl";
const FUTURE: i64 = 4_102_444_800;
const SECRET: &[u8] = b"the-verifier-s-own-secret";

fn segment(value: &serde_json::Value) -> String {
    URL_SAFE_NO_PAD.encode(value.to_string())
}

fn signed(header: &Header, claims: &serde_json::Value, secret: &[u8]) -> SecretString {
    SecretString::new(encode(header, claims, &EncodingKey::from_secret(secret)).expect("encodes"))
}

/// A token whose header no JOSE parser accepts; the parser's message quotes the offending `alg`.
fn hostile_header() -> SecretString {
    let header = segment(&json!({ "alg": EMAIL, "typ": "JWT", "kid": "k1" }));
    let claims = segment(&json!({ "sub": EMAIL, "exp": FUTURE }));
    SecretString::new(format!("{header}.{claims}.c2ln"))
}

async fn serve_jwks() -> SocketAddr {
    let body = Es256::new("k1", 1).jwks();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut scratch = [0u8; 1024];
            let _ = socket.read(&mut scratch).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    addr
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn a_hostile_token_leaves_no_trace() {
    let captured = CapturedLogs::install().expect("the only subscriber");
    let long_kid = format!("{EMAIL}{}", "q".repeat(10_000));

    let addr = serve_jwks().await;
    let remote = JwtVerifier::from_settings(&AuthSettings {
        token_jwks_url: Some(format!("http://{addr}/jwks")),
        token_jwks_refresh_secs: 86_400,
        token_jwks_min_refetch_secs: 3_600,
        ..AuthSettings::default()
    })
    .await
    .expect("remote verifier");
    let mut pinned = Validation::new(Algorithm::HS256);
    pinned.set_issuer(&["https://iam.example.com"]);
    let local = JwtVerifier::new(DecodingKey::from_secret(SECRET), pinned);
    let multi = MultiIssuerVerifier::new([TrustedIssuer::new(
        "https://iam.example.com",
        "https://service.example.com",
        DecodingKey::from_secret(SECRET),
        Algorithm::HS256,
        ["read"],
    )])
    .expect("one issuer");

    let published = Es256::new("k1", 1);
    let impostor = Es256::new("k1", 2);
    let claims = json!({ "sub": EMAIL, "exp": FUTURE });

    let unknown_key = SecretString::new(published.sign(&long_kid, &claims));
    let remote_forged = SecretString::new(impostor.sign("k1", &claims));
    let mistyped = SecretString::new(published.sign(
        "k1",
        &json!({ "sub": "user-1", "exp": FUTURE, "scopes": EMAIL }),
    ));
    let forged = signed(&Header::new(Algorithm::HS256), &claims, b"not-the-secret");
    let foreign_issuer = signed(
        &Header::new(Algorithm::HS256),
        &json!({ "iss": EMAIL, "sub": EMAIL, "exp": FUTURE }),
        b"anyone",
    );

    let refusals = [
        (remote.verify(&unknown_key).await, "remote: unknown kid"),
        (
            remote.verify(&hostile_header()).await,
            "remote: hostile header",
        ),
        (remote.verify(&remote_forged).await, "remote: forged"),
        (remote.verify(&mistyped).await, "remote: mistyped claim"),
        (
            local.verify(&hostile_header()).await,
            "local: hostile header",
        ),
        (local.verify(&forged).await, "local: forged"),
        (local.verify(&foreign_issuer).await, "local: foreign issuer"),
        (multi.verify(&foreign_issuer).await, "multi: foreign issuer"),
        (
            multi.verify(&hostile_header()).await,
            "multi: hostile header",
        ),
    ];

    let expected = [
        AuthError::Invalid(InvalidToken::UnknownKey),
        AuthError::Invalid(InvalidToken::MalformedHeader),
        AuthError::Invalid(InvalidToken::BadSignature),
        AuthError::Invalid(InvalidToken::MalformedClaims),
        AuthError::Invalid(InvalidToken::MalformedHeader),
        AuthError::Invalid(InvalidToken::BadSignature),
        // The local verifier checks the signature before the issuer it pins.
        AuthError::Invalid(InvalidToken::BadSignature),
        AuthError::Untrusted(UntrustedToken::IssuerNotAccepted),
        AuthError::Invalid(InvalidToken::MalformedHeader),
    ];
    for ((refusal, what), expected) in refusals.into_iter().zip(expected) {
        let error = refusal.expect_err(what);
        assert_eq!(
            format!("{error:?}"),
            format!("{expected:?}"),
            "{what} is refused as its class"
        );
        let shown = format!("{error} {error:?}");
        assert!(!shown.contains(EMAIL), "{what}: the error carries {shown}");
    }

    let written = captured.text();
    assert!(
        written.contains("token refused"),
        "the refusals were recorded: {written}"
    );
    assert_eq!(
        written.matches("token refused").count(),
        9,
        "one event per refused token"
    );
    assert!(
        written.contains("refetch=\"started\""),
        "the unknown kid's fetch was recorded: {written}"
    );
    for needle in [EMAIL, &"q".repeat(32)] {
        assert!(
            !written.contains(needle),
            "`{needle}` reached the logs:\n{written}"
        );
    }
}
