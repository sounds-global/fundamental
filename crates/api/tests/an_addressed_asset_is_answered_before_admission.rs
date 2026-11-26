//! A declared file asked for at its exact version is answered before admission.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fundamental_api::{ApiListener, ApiServer};
use fundamental_auth::{AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier};
use fundamental_core::AnyError;
use fundamental_core::address::WebOrigin;
use fundamental_core::secrets::SecretString;
use fundamental_macros::Error;
use fundamental_server::{
    Admission, AdmissionRequest, CredentialSource, Route, Router, ServerConfig, ServerListener,
    StaticAsset,
};
use reqwest::header::{CACHE_CONTROL, CONTENT_LENGTH, ETAG, SET_COOKIE, VARY};

#[derive(Debug, Error)]
enum Refused {
    #[error(code = "TEST_429", message = "Refused at admission", status = 429)]
    Budget,
}

#[derive(Debug, Default)]
struct Refusing {
    asked: Arc<AtomicUsize>,
}

impl Admission for Refusing {
    fn admit(&self, _request: &AdmissionRequest<'_>) -> Result<(), AnyError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Err(Refused::Budget.into())
    }
}

#[derive(Clone)]
struct RefuseEveryToken;

impl PrincipalVerifier for RefuseEveryToken {
    async fn verify(&self, _token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        Err(AuthError::Invalid(InvalidToken::Unrecognised))
    }
}

struct Served {
    listener: ApiListener,
    asked: Arc<AtomicUsize>,
    css: String,
    font: String,
    bundle: String,
}

async fn page(_context: fundamental_server::HttpContext<()>) -> Result<String, AnyError> {
    Ok("<h1>page</h1>".to_owned())
}

fn file(name: &str, bytes: &[u8]) -> StaticAsset {
    StaticAsset::new(Path::new(name), bytes.to_vec())
}

async fn serve() -> Served {
    let router = Router::<()>::new()
        .route(Route::html("/", page))
        .expect("the route registers")
        .static_asset("/_f/fonts/inter.woff2", file("inter.woff2", b"wOF2"))
        .static_asset("/_f/app.css", file("app.css", b"a{}"))
        .static_asset("/_f/app_bg.wasm", file("app_bg.wasm", &[0_u8; 4096]))
        .static_cache_control("/_f/", "no-cache")
        .before_admission("/_f/app.css")
        .expect("served")
        .before_admission("/_f/fonts/")
        .expect("served");
    let addresses = router.asset_addresses();
    let url = |path: &str| addresses.url(path).to_string();
    let (css, font, bundle) = (
        url("/_f/app.css"),
        url("/_f/fonts/inter.woff2"),
        url("/_f/app_bg.wasm"),
    );
    let asked = Arc::new(AtomicUsize::new(0));
    let config = ServerConfig::new("127.0.0.1:0".parse().expect("an ephemeral bind"))
        .with_credential_source(CredentialSource::cookie("session"))
        .with_admission(Refusing {
            asked: Arc::clone(&asked),
        });
    let listener = ApiServer::new(config, (), router)
        .with_verifier(RefuseEveryToken)
        .serve()
        .await
        .expect("binds an ephemeral port");
    Served {
        listener,
        asked,
        css,
        font,
        bundle,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

fn at(served: &Served, path: &str) -> String {
    format!("http://{}{path}", served.listener.local_addr())
}

fn header(response: &reqwest::Response, name: reqwest::header::HeaderName) -> Vec<String> {
    response
        .headers()
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn an_addressed_asset_is_served_when_admission_refuses_everything() {
    let served = serve().await;
    for url in [&served.css, &served.font] {
        let response = client()
            .get(at(&served, url))
            .send()
            .await
            .expect("answers");
        assert_eq!(response.status().as_u16(), 200, "{url}");
        assert_eq!(
            header(&response, CACHE_CONTROL),
            ["public, max-age=31536000, immutable"]
        );
        assert_eq!(header(&response, ETAG).len(), 1);
    }
    assert_eq!(
        served.asked.load(Ordering::SeqCst),
        0,
        "admission was never asked"
    );
}

#[tokio::test]
async fn an_undeclared_addressed_file_meets_admission() {
    let served = serve().await;
    assert!(served.bundle.contains("?v="), "the bundle is addressed");
    let response = client()
        .get(at(&served, &served.bundle))
        .send()
        .await
        .expect("answers");
    assert_eq!(response.status().as_u16(), 429);
    assert_eq!(served.asked.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_unaddressed_stale_or_posted_asset_still_meets_admission() {
    let served = serve().await;
    let stale = "/_f/app.css?v=q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA";
    for url in ["/_f/app.css", stale, "/"] {
        let response = client()
            .get(at(&served, url))
            .send()
            .await
            .expect("answers");
        assert_eq!(response.status().as_u16(), 429, "{url}");
    }
    let posted = client()
        .post(at(&served, &served.css))
        .send()
        .await
        .expect("answers");
    assert_eq!(posted.status().as_u16(), 429);
    assert_eq!(served.asked.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn an_addressed_asset_reads_no_credential_and_carries_no_vary_or_cookie() {
    let served = serve().await;
    let response = client()
        .get(at(&served, &served.css))
        .header("cookie", "session=a; session=b")
        .send()
        .await
        .expect("answers");
    // Two values of the session cookie would be refused had a credential been read.
    assert_eq!(response.status().as_u16(), 200);
    // Nothing a person holds may split the copy a cache keeps.
    let vary = header(&response, VARY);
    assert!(
        vary.iter()
            .flat_map(|value| value.split(','))
            .all(|name| name.trim().eq_ignore_ascii_case("origin")),
        "{vary:?}"
    );
    assert!(header(&response, SET_COOKIE).is_empty());
}

#[tokio::test]
async fn a_head_for_an_addressed_asset_has_its_length_and_no_body() {
    let served = serve().await;
    let response = client()
        .head(at(&served, &served.css))
        .send()
        .await
        .expect("answers");
    assert_eq!(response.status().as_u16(), 200);
    let length: usize = header(&response, CONTENT_LENGTH)
        .first()
        .and_then(|length| length.parse().ok())
        .expect("a length");
    assert!(length > 0);
    assert!(response.bytes().await.expect("a body").is_empty());
    assert_eq!(served.asked.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_matching_if_none_match_is_a_304_before_admission() {
    let served = serve().await;
    let first = client()
        .get(at(&served, &served.css))
        .send()
        .await
        .expect("answers");
    let etag = header(&first, ETAG).remove(0);
    let again = client()
        .get(at(&served, &served.css))
        .header("if-none-match", &etag)
        .send()
        .await
        .expect("answers");
    assert_eq!(again.status().as_u16(), 304);
    assert_eq!(header(&again, ETAG), [etag]);
    assert_eq!(served.asked.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_misdirected_host_is_refused_even_with_a_matching_version() {
    let router = Router::<()>::new()
        .static_asset("/_f/app.css", file("app.css", b"a{}"))
        .before_admission("/_f/app.css")
        .expect("served");
    let url = router.asset_addresses().url("/_f/app.css").to_string();
    let config = ServerConfig::new("127.0.0.1:0".parse().expect("an ephemeral bind"))
        .with_served_origins(vec![
            WebOrigin::parse("https://example.com").expect("an origin"),
        ]);
    let listener = ApiServer::new(config, (), router)
        .serve()
        .await
        .expect("binds");
    let response = client()
        .get(format!("http://{}{url}", listener.local_addr()))
        .header("host", "elsewhere.example")
        .send()
        .await
        .expect("answers");
    assert_eq!(response.status().as_u16(), 421);
}
