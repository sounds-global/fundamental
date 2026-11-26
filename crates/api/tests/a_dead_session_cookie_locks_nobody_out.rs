//! A session cookie the verifier rejects is treated as no cookie, and cleared.

use fundamental_api::{
    AnyError, ApiServer, HttpContext, Router, ServerConfig, ServerListener, ServerResult, auth,
};
use fundamental_auth::{AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier};
use fundamental_core::UserId;
use fundamental_core::secrets::SecretString;
use fundamental_macros::{Error, action};
use fundamental_server::CredentialSource;
use serde::{Deserialize, Serialize};

// Referenced only inside the handlers' signatures, which the macro relocates.
type _Ctx = HttpContext<AppState>;

const SESSION_COOKIE: &str = "__Host-session";

const LIVE: &str = "live";

/// The value the verifier cannot judge because its store did not answer.
const UNJUDGED: &str = "unjudged";

/// The value the verifier cannot judge because the server is broken: not a verdict.
const BROKEN: &str = "broken";

#[derive(Clone)]
struct AppState;

#[derive(Serialize, Deserialize)]
struct Empty {}

#[derive(Serialize, Deserialize)]
struct Who {
    signed_in: bool,
}

/// A store that did not answer: retryable, so never a reason to forget a session.
#[derive(Debug, Error)]
enum StoreError {
    #[error(
        code = "TEST_001",
        message = "The store did not answer",
        status = 503,
        retryable
    )]
    Unavailable,
    #[error(code = "TEST_002", message = "The server is not set up", status = 500)]
    NotSetUp,
}

#[action(GET, "/page")]
async fn page(ctx: HttpContext<AppState>, _input: Empty) -> Result<Who, AnyError> {
    Ok(Who {
        signed_in: ctx.principal_opt().is_some(),
    })
}

#[action(GET, "/account", auth)]
async fn account(_ctx: HttpContext<AppState>, _input: Empty) -> Result<Who, AnyError> {
    Ok(Who { signed_in: true })
}

#[action(POST, "/sign-in")]
async fn sign_in(ctx: HttpContext<AppState>, _input: Empty) -> Result<Who, AnyError> {
    ctx.set_response_header(
        "set-cookie",
        format!("{SESSION_COOKIE}={LIVE}; Path=/; HttpOnly; Secure; SameSite=Lax"),
    );
    Ok(Who { signed_in: true })
}

/// Accepts [`LIVE`], cannot judge [`UNJUDGED`], and rejects everything else.
struct SessionTable;

impl PrincipalVerifier for SessionTable {
    async fn verify(&self, token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        match token.expose().as_str() {
            LIVE => Ok(AuthenticatedPrincipal {
                user_id: UserId::new("account"),
                device_id: None,
                scopes: Vec::new(),
                expires_at: i64::MAX,
            }),
            UNJUDGED => Err(AuthError::Other(AnyError::new(StoreError::Unavailable))),
            BROKEN => Err(AuthError::Other(AnyError::new(StoreError::NotSetUp))),
            _ => Err(AuthError::Invalid(InvalidToken::Unrecognised)),
        }
    }
}

fn router() -> ServerResult<Router<AppState>> {
    Router::new()
        .route(page_route())?
        .route(account_route())?
        .route(sign_in_route())
}

async fn surface(source: CredentialSource) -> fundamental_api::ApiListener {
    let address = "127.0.0.1:0".parse().expect("a loopback address parses");
    ApiServer::new(
        ServerConfig::new(address)
            .with_credential_source(source)
            .with_trusted_write_origins(Vec::new()),
        AppState,
        router().expect("the routes are unique"),
    )
    .with_verifier(SessionTable)
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

fn set_cookies(response: &reqwest::Response) -> Vec<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect()
}

fn header(response: &reqwest::Response, name: &str) -> String {
    response
        .headers()
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join(", ")
}

fn clears_the_session(response: &reqwest::Response) -> bool {
    set_cookies(response).iter().any(|value| {
        value.starts_with(&format!("{SESSION_COOKIE}=;")) && value.contains("Max-Age=0")
    })
}

#[tokio::test]
async fn a_failed_cookie_credential_is_treated_as_absent_and_cleared() {
    let listener = surface(CredentialSource::cookie(SESSION_COOKIE)).await;
    let dead = format!("{SESSION_COOKIE}=revoked");

    let open = client()
        .get(format!("http://{}/page", listener.local_addr()))
        .header("cookie", &dead)
        .send()
        .await
        .expect("the request reaches the surface");
    assert_eq!(
        open.status(),
        200,
        "a public page is served to a browser holding a dead cookie"
    );
    assert!(
        clears_the_session(&open),
        "and the dead cookie is cleared: {:?}",
        set_cookies(&open)
    );
    let who: Who = open.json().await.expect("the page answers");
    assert!(!who.signed_in, "a dead cookie is no principal");

    let guarded = client()
        .get(format!("http://{}/account", listener.local_addr()))
        .header("cookie", &dead)
        .send()
        .await
        .expect("the request reaches the surface");
    assert_eq!(
        guarded.status(),
        401,
        "an authenticated page still asks for a session"
    );
    assert!(clears_the_session(&guarded), "and clears the dead one");
}

#[tokio::test]
async fn a_browser_holding_a_dead_cookie_can_sign_in_again() {
    let listener = surface(CredentialSource::cookie(SESSION_COOKIE)).await;
    let response = client()
        .post(format!("http://{}/sign-in", listener.local_addr()))
        .header("cookie", format!("{SESSION_COOKIE}=revoked"))
        .header("sec-fetch-site", "same-origin")
        .json(&Empty {})
        .send()
        .await
        .expect("the request reaches the surface");

    assert_eq!(response.status(), 200, "the sign-in is served");
    let cookies = set_cookies(&response);
    assert_eq!(
        cookies,
        vec![format!(
            "{SESSION_COOKIE}={LIVE}; Path=/; HttpOnly; Secure; SameSite=Lax"
        )],
        "the new session is set, and no clearing header races it"
    );
}

#[tokio::test]
async fn a_live_cookie_is_neither_refused_nor_cleared() {
    let listener = surface(CredentialSource::cookie(SESSION_COOKIE)).await;
    let response = client()
        .get(format!("http://{}/account", listener.local_addr()))
        .header("cookie", format!("{SESSION_COOKIE}={LIVE}"))
        .send()
        .await
        .expect("the request reaches the surface");

    assert_eq!(response.status(), 200);
    assert!(
        set_cookies(&response).is_empty(),
        "a live session is left alone"
    );
}

#[tokio::test]
async fn a_failed_bearer_credential_is_still_refused() {
    let listener = surface(CredentialSource::bearer_then_cookie(SESSION_COOKIE)).await;
    let response = client()
        .get(format!("http://{}/page", listener.local_addr()))
        .header("authorization", "Bearer revoked")
        .send()
        .await
        .expect("the request reaches the surface");

    assert_eq!(
        response.status(),
        401,
        "a bearer is attached deliberately and told it is bad"
    );
    assert!(
        set_cookies(&response).is_empty(),
        "and no cookie is touched"
    );
    let body = response.text().await.expect("the refusal carries a body");
    assert!(
        body.contains("AUTH_002"),
        "the refusal names the bad token: {body}"
    );
}

#[tokio::test]
async fn a_verifier_that_could_not_answer_is_an_outage_not_a_sign_out() {
    let listener = surface(CredentialSource::cookie(SESSION_COOKIE)).await;
    let response = client()
        .get(format!("http://{}/page", listener.local_addr()))
        .header("cookie", format!("{SESSION_COOKIE}={UNJUDGED}"))
        .send()
        .await
        .expect("the request reaches the surface");

    assert_eq!(
        response.status(),
        503,
        "a store that did not answer is an outage"
    );
    assert!(
        !clears_the_session(&response),
        "and a session nobody judged is never cleared"
    );
    assert_eq!(
        header(&response, "cache-control"),
        "private, no-store",
        "a refusal keyed on a cookie is as private as any answer to one"
    );
    assert!(
        header(&response, "vary")
            .to_ascii_lowercase()
            .contains("cookie"),
        "{}",
        header(&response, "vary")
    );
}

#[tokio::test]
async fn a_broken_server_is_an_error_not_a_sign_out() {
    let listener = surface(CredentialSource::cookie(SESSION_COOKIE)).await;
    let response = client()
        .get(format!("http://{}/page", listener.local_addr()))
        .header("cookie", format!("{SESSION_COOKIE}={BROKEN}"))
        .send()
        .await
        .expect("the request reaches the surface");

    assert_eq!(
        response.status(),
        500,
        "a verifier that is not set up is a server error"
    );
    assert!(
        !clears_the_session(&response),
        "and a session nobody could judge is never cleared"
    );
}
