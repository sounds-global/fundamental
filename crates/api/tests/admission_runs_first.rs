//! Admission runs before the body is read or the credential verified, over a real socket.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fundamental_api::{ApiListener, ApiServer};
use fundamental_auth::{AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier};
use fundamental_core::AnyError;
use fundamental_core::secrets::SecretString;
use fundamental_macros::Error;
use fundamental_server::{
    Admission, AdmissionRequest, HttpContext, Method, Peer, Route, Router, ServerConfig,
    ServerListener,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

#[derive(Debug, Error)]
enum Refused {
    #[error(
        code = "TEST_429",
        message = "Refused at admission",
        status = 429,
        retryable,
        retry_after = retry_after_seconds
    )]
    Budget { retry_after_seconds: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    peer_is_loopback: bool,
    method: Option<Method>,
    forwarded_for: Option<String>,
}

#[derive(Debug, Default)]
struct Recording {
    refuse: bool,
    seen: Mutex<Vec<Seen>>,
}

#[derive(Debug, Clone, Default)]
struct RecordingAdmission(Arc<Recording>);

impl RecordingAdmission {
    fn refusing() -> Self {
        Self(Arc::new(Recording {
            refuse: true,
            seen: Mutex::default(),
        }))
    }

    fn seen(&self) -> Vec<Seen> {
        self.0
            .seen
            .lock()
            .expect("the record is never poisoned")
            .clone()
    }
}

impl Admission for RecordingAdmission {
    fn admit(&self, request: &AdmissionRequest<'_>) -> Result<(), AnyError> {
        self.0
            .seen
            .lock()
            .expect("the record is never poisoned")
            .push(Seen {
                peer_is_loopback: request
                    .peer()
                    .and_then(Peer::address)
                    .is_some_and(|peer| peer.ip().is_loopback()),
                method: request.method(),
                forwarded_for: request.headers().get("x-forwarded-for").map(str::to_owned),
            });
        if self.0.refuse {
            Err(Refused::Budget {
                retry_after_seconds: 17,
            }
            .into())
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Default)]
struct CountingVerifier(Arc<AtomicUsize>);

impl PrincipalVerifier for CountingVerifier {
    async fn verify(&self, _token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(AuthError::Invalid(InvalidToken::Unrecognised))
    }
}

async fn echo(_context: HttpContext<()>, _: ()) -> Result<String, AnyError> {
    Ok("served".to_owned())
}

async fn surface(admission: RecordingAdmission, verifier: CountingVerifier) -> ApiListener {
    let router = Router::<()>::new()
        .route(Route::post("/upload", echo))
        .expect("the route registers");
    ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("an ephemeral bind"))
            .with_admission(admission),
        (),
        router,
    )
    .with_verifier(verifier)
    .serve()
    .await
    .expect("binds an ephemeral port")
}

/// Sends `head` alone; `None` when the server answers nothing within five seconds.
async fn send_head_only(address: SocketAddr, head: &str) -> Option<String> {
    let mut stream = TcpStream::connect(address).await.expect("connects");
    stream
        .write_all(head.as_bytes())
        .await
        .expect("writes the head");
    let mut answer = vec![0_u8; 4096];
    let read = timeout(Duration::from_secs(5), stream.read(&mut answer))
        .await
        .ok()?
        .expect("reads");
    Some(String::from_utf8_lossy(&answer[..read]).into_owned())
}

#[tokio::test]
async fn admission_refuses_before_the_body_is_read_or_a_credential_verified() {
    let admission = RecordingAdmission::refusing();
    let verifier = CountingVerifier::default();
    let listener = surface(admission.clone(), verifier.clone()).await;
    let address = listener.local_addr().address().expect("a bound address");

    let answer = send_head_only(
        address,
        &format!(
            "POST /upload HTTP/1.1\r\nhost: {address}\r\nauthorization: Bearer not-a-token\r\n\
             x-forwarded-for: 203.0.113.7\r\ncontent-type: application/json\r\n\
             content-length: 1000000\r\n\r\n"
        ),
    )
    .await
    .expect("the refusal arrives while the promised body is still unsent");

    assert!(
        answer.starts_with("HTTP/1.1 429"),
        "a refused request is answered with the refusal, not a 401: {answer}"
    );
    assert!(
        answer.to_ascii_lowercase().contains("retry-after: 17"),
        "the refusal's delay reaches the caller: {answer}"
    );
    assert!(answer.contains("TEST_429"), "{answer}");
    assert_eq!(
        verifier.0.load(Ordering::SeqCst),
        0,
        "a refused request's credential was verified"
    );
    assert_eq!(
        admission.seen(),
        vec![Seen {
            peer_is_loopback: true,
            method: Some(Method::Post),
            forwarded_for: Some("203.0.113.7".to_owned()),
        }],
        "admission is told the peer the server saw, the method and the headers"
    );

    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn a_method_the_server_does_not_serve_is_admitted_or_refused_before_it_is_turned_away() {
    let admission = RecordingAdmission::refusing();
    let listener = surface(admission.clone(), CountingVerifier::default()).await;
    let address = listener.local_addr().address().expect("a bound address");

    let answer = send_head_only(
        address,
        &format!("PURGE /upload HTTP/1.1\r\nhost: {address}\r\ncontent-length: 0\r\n\r\n"),
    )
    .await
    .expect("answered");

    assert!(answer.starts_with("HTTP/1.1 429"), "{answer}");
    assert_eq!(
        admission.seen().first().map(|seen| seen.method),
        Some(None),
        "an unserved method reaches admission as None"
    );

    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn an_admitted_request_reaches_the_verifier_and_the_handler_as_before() {
    let admission = RecordingAdmission::default();
    let verifier = CountingVerifier::default();
    let listener = surface(admission.clone(), verifier.clone()).await;
    let url = format!("http://{}/upload", listener.local_addr());
    let client = reqwest::Client::new();

    let with_a_bad_token = client
        .post(&url)
        .bearer_auth("not-a-token")
        .json(&())
        .send()
        .await
        .expect("sent");
    assert_eq!(with_a_bad_token.status(), 401);
    assert_eq!(verifier.0.load(Ordering::SeqCst), 1);

    let without_one = client.post(&url).json(&()).send().await.expect("sent");
    assert_eq!(without_one.status(), 200);
    assert_eq!(without_one.text().await.expect("body"), "\"served\"");
    assert_eq!(admission.seen().len(), 2, "every request met admission");

    listener.stop().await.expect("stops");
}
