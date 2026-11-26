//! Remote JWKS fetching and refetch on an unknown `kid`, against a local HTTP server.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fundamental_auth::{AuthError, InvalidToken, JwtVerifier, PrincipalVerifier};
use fundamental_config::settings::AuthSettings;
use fundamental_core::secrets::SecretString;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

mod support;

use support::Es256;

const FUTURE: i64 = 4_102_444_800; // 2100-01-01

/// A JWKS document publishing the P-256 key `seed` as `kid`.
fn jwks(kid: &str, seed: u8) -> String {
    Es256::new(kid, seed).jwks()
}

/// A token for `sub`, signed by the P-256 key `seed`, naming `kid`.
fn mint(kid: &str, seed: u8, sub: &str) -> SecretString {
    SecretString::new(Es256::new(kid, seed).sign(kid, &json!({ "sub": sub, "exp": FUTURE })))
}

fn ok(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Serves `responses` in turn (the last repeats), each `delay` after its request, counting requests.
async fn serve_raw(responses: Vec<String>, delay: Duration) -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let fetches = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&fetches);
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let n = counted.fetch_add(1, Ordering::SeqCst);
            let response = responses[n.min(responses.len() - 1)].clone();
            tokio::spawn(async move {
                let mut scratch = [0u8; 1024];
                let _ = socket.read(&mut scratch).await;
                tokio::time::sleep(delay).await;
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (addr, fetches)
}

async fn serve(bodies: Vec<String>) -> (SocketAddr, Arc<AtomicUsize>) {
    serve_raw(bodies.iter().map(|body| ok(body)).collect(), Duration::ZERO).await
}

fn settings(url: String) -> AuthSettings {
    AuthSettings {
        token_jwks_url: Some(url),
        token_jwks_refresh_secs: 86_400,
        token_jwks_min_refetch_secs: 3_600,
        ..AuthSettings::default()
    }
}

#[tokio::test]
async fn fetches_a_remote_jwks_and_verifies_a_token() {
    let seed = 1;
    let (addr, _) = serve(vec![jwks("k1", seed)]).await;
    let verifier = JwtVerifier::from_settings(&settings(format!("http://{addr}/jwks")))
        .await
        .expect("build verifier");

    let principal = verifier
        .verify(&mint("k1", seed, "alice"))
        .await
        .expect("verify");
    assert_eq!(principal.user_id.as_str(), "alice");
}

#[tokio::test]
async fn a_rotated_key_is_honoured_after_one_fetch() {
    let seed1 = 1;
    let seed2 = 2;
    let (addr, fetches) = serve(vec![jwks("k1", seed1), jwks("k2", seed2)]).await;
    let verifier = JwtVerifier::from_settings(&settings(format!("http://{addr}/jwks")))
        .await
        .expect("build verifier");

    let principal = verifier
        .verify(&mint("k2", seed2, "bob"))
        .await
        .expect("verify after refetch");
    assert_eq!(principal.user_id.as_str(), "bob");
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        2,
        "startup, then one refetch"
    );
}

#[tokio::test]
async fn a_token_for_a_never_present_kid_is_rejected() {
    let seed = 1;
    let (addr, _) = serve(vec![jwks("k1", seed)]).await;
    let verifier = JwtVerifier::from_settings(&settings(format!("http://{addr}/jwks")))
        .await
        .expect("build verifier");

    let err = verifier
        .verify(&mint("ghost", seed, "mallory"))
        .await
        .expect_err("must reject");
    assert!(matches!(err, AuthError::Invalid(InvalidToken::UnknownKey)));
}

/// Verifies `count` tokens at once, each naming a key nobody publishes.
async fn unknown_kids(verifier: &Arc<JwtVerifier>, seed: u8, count: usize, tag: &str) {
    let tasks: Vec<_> = (0..count)
        .map(|i| {
            let verifier = Arc::clone(verifier);
            let token = mint(&format!("{tag}-{i}"), seed, "mallory");
            tokio::spawn(async move { verifier.verify(&token).await })
        })
        .collect();
    for task in tasks {
        let refused = task.await.expect("task").expect_err("no such key");
        assert!(matches!(
            refused,
            AuthError::Invalid(InvalidToken::UnknownKey)
        ));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_kids_cause_one_fetch_per_interval() {
    let seed = 1;
    let (addr, fetches) = serve(vec![jwks("k1", seed)]).await;
    let verifier = Arc::new(
        JwtVerifier::from_settings(&settings(format!("http://{addr}/jwks")))
            .await
            .expect("build verifier"),
    );
    assert_eq!(fetches.load(Ordering::SeqCst), 1, "the startup fetch");

    unknown_kids(&verifier, seed, 64, "burst").await;
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        2,
        "sixty-four unknown kids at once share one fetch"
    );

    unknown_kids(&verifier, seed, 64, "again").await;
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        2,
        "within the interval, an unknown kid fetches nothing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_requests_do_not_re_arm_the_fetch() {
    let seed = 1;
    let (addr, fetches) = serve_raw(vec![ok(&jwks("k1", seed))], Duration::from_millis(200)).await;
    let verifier = Arc::new(
        JwtVerifier::from_settings(&settings(format!("http://{addr}/jwks")))
            .await
            .expect("build verifier"),
    );

    // Signed first: signing takes long enough that the loop would outlive the fetch.
    let tokens: Vec<_> = (0..64)
        .map(|i| mint(&format!("cancelled-{i}"), seed, "mallory"))
        .collect();
    // Each request is abandoned long before the fetch it caused could answer.
    for token in tokens {
        let abandoned =
            tokio::time::timeout(Duration::from_millis(1), verifier.verify(&token)).await;
        assert!(abandoned.is_err(), "the request was cancelled mid-fetch");
    }
    unknown_kids(&verifier, seed, 1, "patient").await;

    assert_eq!(
        fetches.load(Ordering::SeqCst),
        2,
        "startup, then one fetch however many requests were cancelled"
    );
}

#[tokio::test]
async fn an_oversized_jwks_is_refused() {
    let padded = format!(
        r#"{{"keys":[{}],"pad":"{}"}}"#,
        Es256::new("k1", 1).jwk(),
        "x".repeat(300 * 1024)
    );
    let (addr, _) = serve(vec![padded]).await;
    let refused = JwtVerifier::from_settings(&settings(format!("http://{addr}/jwks"))).await;
    assert!(matches!(refused, Err(AuthError::Jwks(_))));
}

#[tokio::test]
async fn a_redirecting_jwks_endpoint_is_refused() {
    let seed = 1;
    let (target, target_fetches) = serve(vec![jwks("k1", seed)]).await;
    let redirect = format!(
        "HTTP/1.1 302 Found\r\nLocation: http://{target}/jwks\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let (addr, _) = serve_raw(vec![redirect], Duration::ZERO).await;
    let refused = JwtVerifier::from_settings(&settings(format!("http://{addr}/jwks"))).await;
    assert!(matches!(refused, Err(AuthError::Jwks(_))));
    assert_eq!(
        target_fetches.load(Ordering::SeqCst),
        0,
        "the redirect was not followed"
    );
}

#[tokio::test]
async fn a_plain_http_jwks_url_to_another_host_is_refused_at_startup() {
    let refused = JwtVerifier::from_settings(&settings(
        "http://iam.example.com/.well-known/jwks.json".to_owned(),
    ))
    .await;
    assert!(matches!(refused, Err(AuthError::Config(_))));
}
