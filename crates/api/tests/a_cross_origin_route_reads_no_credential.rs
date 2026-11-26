//! A route that answers another origin reads no credential, and trusts that origin on that route alone.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fundamental_api::{
    AnyError, ApiServer, HttpContext, Router, ServerConfig, ServerListener, ServerResult,
};
use fundamental_auth::{AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier};
use fundamental_core::address::WebOrigin;
use fundamental_core::secrets::SecretString;
use fundamental_macros::action;
use fundamental_server::CredentialSource;
use serde::{Deserialize, Serialize};

type _Ctx = HttpContext<AppState>;

const SESSION_COOKIE: &str = "__Host-session";
const SITE: &str = "https://site.test";

#[derive(Clone)]
struct AppState;

#[derive(Serialize, Deserialize)]
struct Seen {
    cookie: bool,
    authorization: bool,
}

fn seen(ctx: &HttpContext<AppState>) -> Seen {
    Seen {
        cookie: ctx.header("cookie").is_some(),
        authorization: ctx.header("authorization").is_some(),
    }
}

#[action(POST, "/chat/{id}/messages", cross_origin = "site")]
async fn post_message(ctx: HttpContext<AppState>, _input: ()) -> Result<Seen, AnyError> {
    Ok(seen(&ctx))
}

#[action(GET, "/chat/{id}", cross_origin = "site")]
async fn read_chat(ctx: HttpContext<AppState>, _input: ()) -> Result<Seen, AnyError> {
    Ok(seen(&ctx))
}

#[action(POST, "/account")]
async fn change_account(ctx: HttpContext<AppState>, _input: ()) -> Result<Seen, AnyError> {
    Ok(seen(&ctx))
}

#[derive(Clone, Default)]
struct CountingVerifier(Arc<AtomicUsize>);

impl PrincipalVerifier for CountingVerifier {
    async fn verify(&self, _token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(AuthError::Invalid(InvalidToken::Unrecognised))
    }
}

fn router() -> ServerResult<Router<AppState>> {
    Router::new()
        .route(post_message_route())?
        .route(read_chat_route())?
        .route(change_account_route())
}

fn config() -> ServerConfig {
    let address = "127.0.0.1:0".parse().expect("a loopback address parses");
    ServerConfig::new(address)
        .with_credential_source(CredentialSource::cookie(SESSION_COOKIE))
        .with_cross_origin("site", vec![WebOrigin::parse(SITE).expect("an origin")])
}

async fn app(verifier: CountingVerifier) -> fundamental_api::ApiListener {
    ApiServer::new(config(), AppState, router().expect("unique routes"))
        .with_verifier(verifier)
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

/// What the site's panel sends: its origin, `same-site`, and the app's cookie if it has one.
fn from_the_site(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request
        .header("origin", SITE)
        .header("sec-fetch-site", "same-site")
        .header("cookie", format!("{SESSION_COOKIE}=someones-session"))
        .header("authorization", "Bearer someones-token")
}

#[tokio::test]
async fn the_site_writes_on_its_route_and_no_credential_reaches_it() {
    let verifier = CountingVerifier::default();
    let listener = app(verifier.clone()).await;
    let response =
        from_the_site(client().post(format!("http://{}/chat/1/messages", listener.local_addr())))
            .json(&())
            .send()
            .await
            .expect("the request completes");

    assert_eq!(response.status(), 200);
    assert!(
        response.headers().get("set-cookie").is_none(),
        "a credential it never read is never cleared"
    );
    let seen: Seen = response.json().await.expect("a JSON body");
    assert!(!seen.cookie, "the cookie never reaches the handler");
    assert!(!seen.authorization, "nor the bearer");
    assert_eq!(
        verifier.0.load(Ordering::SeqCst),
        0,
        "no credential was verified"
    );
}

#[tokio::test]
async fn another_origin_cannot_write_on_the_route() {
    let listener = app(CountingVerifier::default()).await;
    let response = client()
        .post(format!("http://{}/chat/1/messages", listener.local_addr()))
        .header("origin", "https://evil.example")
        .header("sec-fetch-site", "cross-site")
        .json(&())
        .send()
        .await
        .expect("the request completes");

    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn a_page_elsewhere_cannot_spend_a_read_on_the_route() {
    let listener = app(CountingVerifier::default()).await;
    let read = |site: &'static str, origin: Option<&'static str>| {
        let request = client()
            .get(format!("http://{}/chat/1", listener.local_addr()))
            .header("sec-fetch-site", site);
        match origin {
            Some(origin) => request.header("origin", origin),
            None => request,
        }
    };
    // An `<img>` on any page sends no `Origin`; a sibling's `fetch` sends its own.
    for (site, origin) in [
        ("cross-site", None),
        ("same-site", None),
        ("same-site", Some("https://other.site.test")),
        ("cross-site", Some("null")),
    ] {
        let response = read(site, origin)
            .send()
            .await
            .expect("the request completes");
        assert_eq!(response.status(), 403, "{site} {origin:?}");
        let body: serde_json::Value = response.json().await.expect("a JSON body");
        assert_eq!(body["error"], "API_031", "{site} {origin:?}: {body}");
    }
    for (site, origin) in [
        ("same-site", Some(SITE)),
        ("same-origin", None),
        ("none", None),
    ] {
        let response = read(site, origin)
            .send()
            .await
            .expect("the request completes");
        assert_eq!(response.status(), 200, "{site} {origin:?}");
    }
    let unsigned = client()
        .get(format!("http://{}/chat/1", listener.local_addr()))
        .send()
        .await
        .expect("the request completes");
    assert_eq!(unsigned.status(), 200, "no browser's page sent it");
}

#[tokio::test]
async fn a_write_that_names_no_origin_is_refused_on_the_route() {
    let listener = app(CountingVerifier::default()).await;
    let response = client()
        .post(format!("http://{}/chat/1/messages", listener.local_addr()))
        .json(&())
        .send()
        .await
        .expect("the request completes");

    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn the_site_cannot_write_on_any_other_route() {
    let listener = app(CountingVerifier::default()).await;
    let response =
        from_the_site(client().post(format!("http://{}/account", listener.local_addr())))
            .json(&())
            .send()
            .await
            .expect("the request completes");

    assert_eq!(response.status(), 403);
}

async fn preflight(
    listener: &fundamental_api::ApiListener,
    path: &str,
    origin: &str,
) -> reqwest::Response {
    client()
        .request(
            reqwest::Method::OPTIONS,
            format!("http://{}{path}", listener.local_addr()),
        )
        .header("origin", origin)
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type")
        .send()
        .await
        .expect("the preflight completes")
}

fn allowed_origin(response: &reqwest::Response) -> Option<&str> {
    response
        .headers()
        .get("access-control-allow-origin")
        .and_then(|value| value.to_str().ok())
}

#[tokio::test]
async fn cors_allows_the_site_on_its_route_and_nowhere_else() {
    let listener = app(CountingVerifier::default()).await;

    let chat = preflight(&listener, "/chat/1/messages", SITE).await;
    assert_eq!(allowed_origin(&chat), Some(SITE));
    assert!(
        chat.headers()
            .get("access-control-allow-credentials")
            .is_none(),
        "a route that reads no credential never asks the browser for one"
    );

    let account = preflight(&listener, "/account", SITE).await;
    assert_eq!(
        allowed_origin(&account),
        None,
        "the site is allowed on no other route"
    );

    let evil = preflight(&listener, "/chat/1/messages", "https://evil.example").await;
    assert_eq!(
        allowed_origin(&evil),
        None,
        "only the named origins are allowed"
    );

    let read = from_the_site(client().get(format!("http://{}/chat/1", listener.local_addr())))
        .send()
        .await
        .expect("the read completes");
    assert_eq!(read.status(), 200);
    assert_eq!(
        allowed_origin(&read),
        Some(SITE),
        "the site may read the answer"
    );
}

#[action(POST, "/elsewhere", cross_origin = "partner")]
async fn unnamed_set(_ctx: HttpContext<AppState>, _input: ()) -> Result<(), AnyError> {
    Ok(())
}

#[action(POST, "/mine", fundamental_server::auth, cross_origin = "site")]
async fn needs_a_principal(_ctx: HttpContext<AppState>, _input: ()) -> Result<(), AnyError> {
    Ok(())
}

#[tokio::test]
async fn a_misdeclared_cross_origin_route_refuses_to_start() {
    for route in [unnamed_set_route(), needs_a_principal_route()] {
        let refused = ApiServer::new(
            config(),
            AppState,
            Router::new().route(route).expect("one route"),
        )
        .with_verifier(CountingVerifier::default())
        .serve()
        .await;
        let error = refused.err().expect("the server refuses to start");
        assert!(
            error.to_string().contains("cross-origin"),
            "names what is wrong: {error}"
        );
    }
}
