//! The cross-site guard covers the route that hands out a cookie, not only those that read one.

use fundamental_api::{
    AnyError, ApiServer, HttpContext, Router, ServerConfig, ServerListener, ServerResult,
};
use fundamental_auth::{AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier};
use fundamental_core::secrets::SecretString;
use fundamental_macros::action;
use fundamental_server::CredentialSource;
use serde::{Deserialize, Serialize};

// Referenced only inside the handler's signature, which the macro relocates.
type _Ctx = HttpContext<AppState>;

const SESSION_COOKIE: &str = "__Host-session";

#[derive(Clone)]
struct AppState;

#[derive(Serialize, Deserialize)]
struct Proof {
    assertion: String,
}

#[derive(Serialize, Deserialize)]
struct SignedIn {
    next: String,
}

/// A door: it presents no credential and its reply mints one.
#[action(POST, "/login/session")]
async fn open_session(ctx: HttpContext<AppState>, input: Proof) -> Result<SignedIn, AnyError> {
    let _ = input.assertion;
    ctx.set_response_header(
        "set-cookie",
        format!("{SESSION_COOKIE}=minted; Path=/; HttpOnly; Secure; SameSite=Lax"),
    );
    Ok(SignedIn {
        next: "/".to_owned(),
    })
}

/// Never consulted here, but a cookie credential source refuses to boot without one.
struct RejectingVerifier;

impl PrincipalVerifier for RejectingVerifier {
    async fn verify(&self, _token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        Err(AuthError::Invalid(InvalidToken::Unrecognised))
    }
}

fn router() -> ServerResult<Router<AppState>> {
    Router::new().route(open_session_route())
}

async fn cookie_surface() -> fundamental_api::ApiListener {
    let address = "127.0.0.1:0".parse().expect("a loopback address parses");
    ApiServer::new(
        ServerConfig::new(address)
            .with_credential_source(CredentialSource::cookie(SESSION_COOKIE))
            .with_trusted_write_origins(Vec::new()),
        AppState,
        router().expect("the route is unique"),
    )
    .with_verifier(RejectingVerifier)
    .serve()
    .await
    .expect("binds an ephemeral port")
}

async fn bearer_surface() -> fundamental_api::ApiListener {
    let address = "127.0.0.1:0".parse().expect("a loopback address parses");
    ApiServer::new(
        ServerConfig::new(address),
        AppState,
        router().expect("the route is unique"),
    )
    .serve()
    .await
    .expect("binds an ephemeral port")
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("a client builds from defaults")
}

fn proof() -> Proof {
    Proof {
        assertion: "a proof this surface never inspects".to_owned(),
    }
}

fn mints_a_session(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|value| value.starts_with(SESSION_COOKIE))
}

#[tokio::test]
async fn a_cross_site_post_to_the_door_is_refused() {
    let listener = cookie_surface().await;
    let response = client()
        .post(format!("http://{}/login/session", listener.local_addr()))
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://evil.example")
        .json(&proof())
        .send()
        .await
        .expect("the request reaches the surface");

    assert_eq!(response.status(), 403, "a cross-site sign-in is refused");
    assert!(
        !mints_a_session(&response),
        "no session is minted for a request that could not prove same-origin intent"
    );
    let body = response.text().await.expect("the refusal carries a body");
    assert!(
        body.contains("API_015"),
        "the refusal is the cross-site-write code rather than a generic error: {body}"
    );
}

#[tokio::test]
async fn a_same_origin_post_to_the_door_still_opens_it() {
    let listener = cookie_surface().await;
    let response = client()
        .post(format!("http://{}/login/session", listener.local_addr()))
        .header("sec-fetch-site", "same-origin")
        .json(&proof())
        .send()
        .await
        .expect("the request reaches the surface");

    assert_eq!(response.status(), 200);
    assert!(mints_a_session(&response), "the person is signed in");
}

#[tokio::test]
async fn a_reply_that_hands_out_a_credential_is_never_cached() {
    let listener = cookie_surface().await;
    let response = client()
        .post(format!("http://{}/login/session", listener.local_addr()))
        .header("sec-fetch-site", "same-origin")
        .json(&proof())
        .send()
        .await
        .expect("the request reaches the surface");

    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    assert!(
        response
            .headers()
            .get("vary")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().contains("cookie")),
        "the reply varies on the cookie it is about to set"
    );
}

#[tokio::test]
async fn a_bearer_surface_is_not_guarded() {
    let listener = bearer_surface().await;
    let response = client()
        .post(format!("http://{}/login/session", listener.local_addr()))
        .json(&proof())
        .send()
        .await
        .expect("the request reaches the surface");

    assert_eq!(
        response.status(),
        200,
        "a surface that reads no cookie is untouched by the cookie guard"
    );
}
