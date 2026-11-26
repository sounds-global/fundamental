//! The `#[action]` macro end to end, over real HTTP.

use std::sync::{Mutex, OnceLock};

use fundamental_api::{
    AnyError, ApiServer, HttpContext, MiddlewareResult, Next, Router, ServerConfig, ServerListener,
    auth, decode_body, encode_body,
};
use fundamental_auth::{AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier};
use fundamental_core::secrets::SecretString;
use fundamental_macros::{action, middleware};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState {
    suffix: &'static str,
}

#[derive(Serialize, Deserialize)]
struct EchoIn {
    message: String,
}

#[derive(Serialize, Deserialize)]
struct EchoOut {
    echoed: String,
}

#[action(POST, "/echo")]
async fn echo(ctx: HttpContext<AppState>, input: EchoIn) -> Result<EchoOut, AnyError> {
    Ok(EchoOut {
        echoed: format!("{}{}", input.message, ctx.state().suffix),
    })
}

#[derive(Serialize, Deserialize)]
struct AcceptOut {
    queued: bool,
}

#[action(POST, "/accept")]
async fn accept(ctx: HttpContext<AppState>, _input: ()) -> Result<AcceptOut, AnyError> {
    ctx.set_response_status(202);
    Ok(AcceptOut { queued: true })
}

#[action(POST, "/ack", content_type = "application/json", raw_input)]
async fn ack(ctx: HttpContext<AppState>, body: String) -> Result<String, AnyError> {
    let _ = body;
    ctx.set_response_status(202);
    Ok(String::new())
}

/// Binary in, binary out: an OHTTP gateway's body is ciphertext, never valid UTF-8.
#[action(
    POST,
    "/binary",
    content_type = "application/octet-stream",
    raw_input,
    bytes
)]
async fn binary(_ctx: HttpContext<AppState>, body: Vec<u8>) -> Result<Vec<u8>, AnyError> {
    let mut out = vec![0xffu8];
    out.extend_from_slice(&body);
    Ok(out)
}

#[derive(Serialize, Deserialize)]
struct MeResponse {
    user: String,
}

#[action(GET, "/me", auth)]
async fn me(ctx: HttpContext<AppState>, _input: ()) -> Result<MeResponse, AnyError> {
    let principal = ctx.principal()?;
    Ok(MeResponse {
        user: principal.user_id.to_string(),
    })
}

async fn start(router: Router<AppState>) -> fundamental_api::ApiListener {
    ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("addr")),
        AppState { suffix: "!" },
        router,
    )
    .serve()
    .await
    .expect("binds an ephemeral port")
}

/// Accepts nothing, so the `auth` gate is exercised rather than a missing verifier.
struct RejectingVerifier;

impl PrincipalVerifier for RejectingVerifier {
    async fn verify(&self, _token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        Err(AuthError::Invalid(InvalidToken::Unrecognised))
    }
}

async fn start_with_verifier(router: Router<AppState>) -> fundamental_api::ApiListener {
    ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("addr")),
        AppState { suffix: "!" },
        router,
    )
    .with_verifier(RejectingVerifier)
    .serve()
    .await
    .expect("binds an ephemeral port")
}

#[tokio::test]
async fn generated_route_round_trips() {
    let router = Router::new().route(echo_route()).expect("unique route");
    let listener = start(router).await;
    let url = format!("http://{}/echo", listener.local_addr());

    let response = reqwest::Client::new()
        .post(&url)
        .json(&serde_json::json!({ "message": "hi" }))
        .send()
        .await
        .expect("request sent");
    assert_eq!(response.status(), 200);
    let out: EchoOut = response.json().await.expect("json body");
    assert_eq!(out.echoed, "hi!");

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_handler_returns_a_non_200_success_status_on_the_wire() {
    let router = Router::new().route(accept_route()).expect("unique route");
    let listener = start(router).await;
    let url = format!("http://{}/accept", listener.local_addr());

    let response = reqwest::Client::new()
        .post(&url)
        .send()
        .await
        .expect("request sent");
    assert_eq!(response.status(), 202, "the opted-in 202 rides the wire");
    let out: AcceptOut = response.json().await.expect("json body");
    assert!(out.queued, "the body is still serialized normally");

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_raw_input_handler_acks_202_with_an_empty_body() {
    let router = Router::new().route(ack_route()).expect("unique route");
    let listener = start(router).await;
    let url = format!("http://{}/ack", listener.local_addr());

    let response = reqwest::Client::new()
        .post(&url)
        .body("a-raw-security-event-token")
        .send()
        .await
        .expect("request sent");
    assert_eq!(
        response.status(),
        202,
        "the raw-input ACK rides the wire as 202"
    );
    let body = response.text().await.expect("text body");
    assert!(body.is_empty(), "a status-only ACK carries no body");

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_binary_handler_round_trips_bytes_that_are_not_utf8() {
    let router = Router::new().route(binary_route()).expect("unique route");
    let listener = start(router).await;
    let url = format!("http://{}/binary", listener.local_addr());

    // Deliberately invalid UTF-8: a lone continuation byte and an unpaired lead.
    let payload = vec![0xff_u8, 0xfe, 0x00, 0x80, 0x41];
    let response = reqwest::Client::new()
        .post(&url)
        .body(payload.clone())
        .send()
        .await
        .expect("request sent");

    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/octet-stream"),
        "the declared media type rides the response"
    );

    let body = response.bytes().await.expect("binary body");
    let mut expected = vec![0xff_u8];
    expected.extend_from_slice(&payload);
    assert_eq!(
        body.to_vec(),
        expected,
        "every byte survives in both directions"
    );

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn auth_middleware_rejects_unauthenticated_request() {
    // A verifier is configured: without one the route is unsatisfiable and the server refuses it.
    let router = Router::new().route(me_route()).expect("unique route");
    let listener = start_with_verifier(router).await;
    let url = format!("http://{}/me", listener.local_addr());

    let response = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .expect("request sent");
    assert_eq!(response.status(), 401);

    listener.stop().await.expect("stops cleanly");
}

fn order_log() -> &'static Mutex<Vec<&'static str>> {
    static LOG: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    LOG.get_or_init(|| Mutex::new(Vec::new()))
}

#[middleware]
async fn record_first<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
    order_log().lock().expect("lock").push("first");
    next.run(ctx, body).await
}

#[middleware]
async fn record_second_and_shout<S>(
    ctx: HttpContext<S>,
    body: Vec<u8>,
    next: Next<S>,
) -> MiddlewareResult {
    order_log().lock().expect("lock").push("second");
    let rewritten = match decode_body::<EchoIn>(&body)? {
        Some(input) => encode_body(&EchoIn {
            message: input.message.to_uppercase(),
        })?,
        None => body,
    };
    next.run(ctx, rewritten).await
}

#[action(POST, "/loud", record_first, record_second_and_shout)]
async fn loud(ctx: HttpContext<AppState>, input: EchoIn) -> Result<EchoOut, AnyError> {
    Ok(EchoOut {
        echoed: format!("{}{}", input.message, ctx.state().suffix),
    })
}

#[tokio::test]
async fn middlewares_run_in_declared_order_and_can_rewrite_the_body() {
    order_log().lock().expect("lock").clear();
    let router = Router::new().route(loud_route()).expect("unique route");
    let listener = start(router).await;
    let url = format!("http://{}/loud", listener.local_addr());

    let response = reqwest::Client::new()
        .post(&url)
        .json(&serde_json::json!({ "message": "hi" }))
        .send()
        .await
        .expect("request sent");
    assert_eq!(response.status(), 200);
    let out: EchoOut = response.json().await.expect("json body");
    assert_eq!(out.echoed, "HI!");

    assert_eq!(*order_log().lock().expect("lock"), vec!["first", "second"]);

    listener.stop().await.expect("stops cleanly");
}
