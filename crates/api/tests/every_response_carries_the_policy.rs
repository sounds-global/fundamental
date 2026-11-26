//! Every response the server produces carries its security headers exactly once, over a real socket.

use std::sync::Arc;
use std::time::Duration;

use fundamental_api::{ApiListener, ApiServer};
use fundamental_auth::{AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier};
use fundamental_core::AnyError;
use fundamental_core::address::{AddressSpace, LocalePrefixSet, SlugTable};
use fundamental_core::secrets::SecretString;
use fundamental_macros::Error;
use fundamental_server::{
    Admission, AdmissionRequest, ContentSecurityPolicy, CredentialSource, Hsts, HttpContext,
    Indexing, Method as ServerMethod, RequestBodyStream, Route, Router, SecurityHeaders,
    ServerConfig, ServerListener, StaticAsset, StreamBounds, auth,
};
use reqwest::header::HeaderMap;
use reqwest::{Client, Method, redirect};

fn policy() -> SecurityHeaders {
    SecurityHeaders::api()
        .with_hsts(Hsts::preload())
        .with_csp(ContentSecurityPolicy::strict())
        .with_indexing(Indexing::Private)
}

fn owned() -> Vec<(&'static str, String)> {
    policy()
        .headers()
        .into_iter()
        .map(|header| (header.name(), header.value().to_owned()))
        .collect()
}

#[derive(Debug, Error)]
enum Refused {
    #[error(code = "TEST_429", message = "Refused at admission", status = 429)]
    AtTheDoor,
}

#[derive(Debug, Clone)]
struct RefuseOnRequest;

impl Admission for RefuseOnRequest {
    fn admit(&self, request: &AdmissionRequest<'_>) -> Result<(), AnyError> {
        match request.headers().get("x-refuse") {
            Some(_) => Err(Refused::AtTheDoor.into()),
            None => Ok(()),
        }
    }
}

#[derive(Clone)]
struct RefuseEveryToken;

impl PrincipalVerifier for RefuseEveryToken {
    async fn verify(&self, _token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        Err(AuthError::Invalid(InvalidToken::Unrecognised))
    }
}

async fn page(_: HttpContext<()>) -> Result<String, AnyError> {
    Ok("<h1>page</h1>".to_owned())
}

async fn json(_: HttpContext<()>, _: ()) -> Result<String, AnyError> {
    Ok("json".to_owned())
}

async fn slow(_: HttpContext<()>, _: ()) -> Result<String, AnyError> {
    tokio::time::sleep(Duration::from_secs(5)).await;
    Ok("too late".to_owned())
}

#[allow(clippy::panic)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
async fn drains(_: HttpContext<()>, mut body: RequestBodyStream) -> Result<u64, AnyError> {
    let mut total = 0_u64;
    while let Some(chunk) = body.next_chunk().await {
        total += chunk.map_err(AnyError::new)?.len() as u64;
    }
    Ok(total)
}

async fn panics(_: HttpContext<()>, _: ()) -> Result<String, AnyError> {
    panic!("a handler that panics");
}

async fn sets_its_own(context: HttpContext<()>) -> Result<String, AnyError> {
    context.set_response_header("Strict-Transport-Security", "max-age=0");
    context.set_response_header("X-Frame-Options", "ALLOWALL");
    context.set_response_header("Content-Security-Policy", "sandbox");
    context.set_response_header("X-Robots-Tag", "noarchive");
    Ok("<p>upload</p>".to_owned())
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn surface() -> ApiListener {
    let prefixes = LocalePrefixSet::new()
        .with("en", "en".parse().expect("tag"))
        .expect("en");
    let slugs = SlugTable::builder()
        .page("/", &[])
        .page("/page", &[])
        .build()
        .expect("slugs");
    let space = AddressSpace::new(prefixes, slugs)
        .with_default_language("nl".parse().expect("tag"))
        .expect("nl");
    let router = Router::<()>::new()
        .with_addresses(Arc::new(space))
        .route(Route::html("/page", page))
        .expect("page")
        .route(Route::html("/upload", sets_its_own))
        .expect("upload")
        .route(Route::get("/json", json))
        .expect("json")
        .route(Route::post("/write", json).middleware(auth))
        .expect("write")
        .route(Route::get("/slow", slow))
        .expect("slow")
        .route(Route::get("/panics", panics))
        .expect("panics")
        .route(Route::post("/upload-body", json))
        .expect("body")
        .route(Route::streamed(
            ServerMethod::Post,
            "/streamed",
            StreamBounds::new(16).with_idle(Duration::from_millis(200)),
            drains,
        ))
        .expect("streamed")
        .route(Route::streamed(
            ServerMethod::Post,
            "/streamed-full",
            StreamBounds::new(16)
                .with_max_in_flight(1)
                .with_idle(Duration::from_secs(30)),
            drains,
        ))
        .expect("full")
        .not_found(Route::html("/not-found", page))
        .static_asset(
            "/app.js",
            StaticAsset::new(std::path::Path::new("app.js"), b"1".to_vec()),
        )
        .document(
            "/robots.txt",
            StaticAsset::typed(b"User-agent: *\n".to_vec(), "text/plain; charset=utf-8"),
        )
        .expect("robots");
    let config = ServerConfig::new("127.0.0.1:0".parse().expect("addr"))
        .with_request_timeout(Duration::from_millis(100))
        .with_max_body_size(16)
        .with_cors_allowed_origins(vec!["https://app.example.nl".to_owned()])
        .with_credential_source(CredentialSource::cookie("__Host-session"))
        .with_admission(RefuseOnRequest)
        .with_security_headers(policy());
    ApiServer::new(config, (), router)
        .with_verifier(RefuseEveryToken)
        .serve()
        .await
        .expect("serves")
}

struct Case {
    class: &'static str,
    method: Method,
    path: &'static str,
    headers: Vec<(&'static str, &'static str)>,
    body: Option<Vec<u8>>,
    status: u16,
}

impl Case {
    fn new(class: &'static str, method: Method, path: &'static str, status: u16) -> Self {
        Self {
            class,
            method,
            path,
            headers: Vec::new(),
            body: None,
            status,
        }
    }

    fn header(mut self, name: &'static str, value: &'static str) -> Self {
        self.headers.push((name, value));
        self
    }

    fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }
}

fn client() -> Client {
    Client::builder()
        .redirect(redirect::Policy::none())
        .build()
        .expect("client")
}

fn assert_carries_the_policy(class: &str, headers: &HeaderMap) {
    for (name, value) in owned() {
        let values: Vec<&str> = headers
            .get_all(name)
            .iter()
            .map(|value| value.to_str().expect("ascii"))
            .collect();
        assert_eq!(
            values.iter().filter(|seen| **seen == value).count(),
            1,
            "{class}: {name} must carry the policy's value exactly once, found {values:?}"
        );
    }
}

#[tokio::test]
async fn every_response_the_server_emits_carries_the_policy() {
    let listener = surface().await;
    let base = format!("http://{}", listener.local_addr());
    let client = client();
    let html = "text/html";
    let cases = [
        Case::new("a page", Method::GET, "/page", 200).header("accept", html),
        Case::new("an action", Method::GET, "/json", 200),
        Case::new("an asset", Method::GET, "/app.js", 200),
        Case::new("a document", Method::GET, "/robots.txt", 200),
        Case::new("a canonical redirect", Method::GET, "/en//page", 301).header("accept", html),
        Case::new("the 404 envelope", Method::GET, "/nowhere", 404),
        Case::new("the not-found page", Method::GET, "/nowhere", 404).header("accept", html),
        Case::new("a 405", Method::DELETE, "/page", 405).header("sec-fetch-site", "same-origin"),
        Case::new("a 408", Method::GET, "/slow", 408),
        Case::new("a 413", Method::POST, "/upload-body", 413)
            .header("sec-fetch-site", "same-origin")
            .body(vec![b'x'; 1024]),
        Case::new("a streamed 413", Method::POST, "/streamed", 413)
            .header("sec-fetch-site", "same-origin")
            .body(vec![b'x'; 1024]),
        Case::new("a panic", Method::GET, "/panics", 500),
        Case::new("an admission refusal", Method::GET, "/page", 429).header("x-refuse", "1"),
        Case::new("a cross-site write", Method::POST, "/write", 403)
            .header("cookie", "__Host-session=t")
            .header("sec-fetch-site", "cross-site")
            .header("origin", "https://evil.example"),
        Case::new("a failed credential", Method::POST, "/write", 401)
            .header("cookie", "__Host-session=t")
            .header("sec-fetch-site", "same-origin"),
        Case::new("a CORS preflight", Method::OPTIONS, "/json", 200)
            .header("origin", "https://app.example.nl")
            .header("access-control-request-method", "GET"),
        Case::new("a HEAD", Method::HEAD, "/page", 200).header("accept", html),
    ];
    for case in cases {
        let mut request = client.request(case.method, format!("{base}{}", case.path));
        for (name, value) in case.headers {
            request = request.header(name, value);
        }
        if let Some(body) = case.body {
            request = request.body(body);
        }
        let response = request.send().await.expect(case.class);
        assert_eq!(response.status().as_u16(), case.status, "{}", case.class);
        assert_carries_the_policy(case.class, response.headers());
    }
    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn a_handler_cannot_weaken_a_header_but_its_csp_tightens() {
    let listener = surface().await;
    let response = client()
        .get(format!("http://{}/upload", listener.local_addr()))
        .header("accept", "text/html")
        .send()
        .await
        .expect("upload");
    let headers = response.headers();
    assert_carries_the_policy("a handler that sets its own", headers);
    let all = |name: &str| -> Vec<String> {
        headers
            .get_all(name)
            .iter()
            .map(|value| value.to_str().expect("ascii").to_owned())
            .collect()
    };
    assert_eq!(
        all("strict-transport-security"),
        ["max-age=63072000; includeSubDomains; preload"]
    );
    assert_eq!(all("x-frame-options"), ["DENY"]);
    assert!(
        all("content-security-policy").contains(&"sandbox".to_owned()),
        "the handler's stricter CSP is kept beside the policy's"
    );
    assert!(all("x-robots-tag").contains(&"noarchive".to_owned()));
    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn a_forged_host_or_forwarded_header_changes_no_byte() {
    let listener = surface().await;
    let base = format!("http://{}", listener.local_addr());
    let client = client();
    for path in ["/robots.txt", "/page", "/nowhere"] {
        let plain = client
            .get(format!("{base}{path}"))
            .header("accept", "text/html")
            .send()
            .await
            .expect("plain");
        let forged = client
            .get(format!("{base}{path}"))
            .header("accept", "text/html")
            .header("host", "evil.example")
            .header("x-forwarded-host", "evil.example")
            .header("x-forwarded-proto", "http")
            .header("forwarded", "host=evil.example;proto=http")
            .send()
            .await
            .expect("forged");
        for (name, _) in owned() {
            assert_eq!(
                plain.headers().get_all(name).iter().collect::<Vec<_>>(),
                forged.headers().get_all(name).iter().collect::<Vec<_>>(),
                "{path}: {name}"
            );
        }
        let plain_body = plain.bytes().await.expect("body");
        let forged_body = forged.bytes().await.expect("body");
        assert_eq!(plain_body, forged_body, "{path}");
        assert!(
            !String::from_utf8_lossy(&forged_body).contains("evil.example"),
            "{path}: the forged host reached the body"
        );
    }
    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn a_server_with_no_policy_sends_the_api_default() {
    let router = Router::<()>::new()
        .route(Route::get("/json", json))
        .expect("json");
    let listener = ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("addr")),
        (),
        router,
    )
    .serve()
    .await
    .expect("serves");
    let response = client()
        .get(format!("http://{}/nowhere", listener.local_addr()))
        .send()
        .await
        .expect("404");
    let headers = response.headers();
    assert_eq!(
        headers.get("x-frame-options").map(|v| v.as_bytes()),
        Some(&b"DENY"[..])
    );
    assert_eq!(
        headers.get("content-security-policy").map(|v| v.as_bytes()),
        Some(&b"frame-ancestors 'none'"[..])
    );
    assert_eq!(
        headers.get("x-content-type-options").map(|v| v.as_bytes()),
        Some(&b"nosniff"[..])
    );
    assert!(headers.get("strict-transport-security").is_none());
    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn a_misdirected_request_carries_the_policy() {
    let served =
        fundamental_core::address::WebOrigin::parse("https://files.example.nl").expect("an origin");
    let listener = ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("addr"))
            .with_security_headers(policy())
            .with_served_origins(vec![served]),
        (),
        Router::new(),
    )
    .serve()
    .await
    .expect("serves");
    let response = client()
        .get(format!("http://{}/anything", listener.local_addr()))
        .header("host", "app.example.nl")
        .send()
        .await
        .expect("sent");
    assert_eq!(response.status(), 421);
    assert_carries_the_policy("a 421", response.headers());
    listener.stop().await.expect("stops");
}

#[tokio::test]
async fn a_streamed_body_cut_for_going_idle_carries_the_policy() {
    let listener = surface().await;
    let (held, chunks) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(1);
    held.send(Ok(b"ab".to_vec())).await.expect("sent");
    let response = client()
        .post(format!("http://{}/streamed", listener.local_addr()))
        .header("sec-fetch-site", "same-origin")
        .body(reqwest::Body::wrap_stream(futures::stream::unfold(
            chunks,
            |mut chunks| async move { chunks.recv().await.map(|chunk| (chunk, chunks)) },
        )))
        .send()
        .await
        .expect("answered while the body is held");
    assert_eq!(response.status().as_u16(), 408);
    assert_carries_the_policy("a streamed 408", response.headers());
    drop(held);
    listener.stop().await.expect("stops");
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_full_streamed_route_carries_the_policy() {
    let listener = surface().await;
    let url = format!("http://{}/streamed-full", listener.local_addr());
    let client = client();
    let (held, chunks) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(1);
    held.send(Ok(b"a".to_vec())).await.expect("sent");
    let holding = tokio::spawn({
        let (client, url) = (client.clone(), url.clone());
        async move {
            client
                .post(url)
                .header("sec-fetch-site", "same-origin")
                .body(reqwest::Body::wrap_stream(futures::stream::unfold(
                    chunks,
                    |mut chunks| async move { chunks.recv().await.map(|chunk| (chunk, chunks)) },
                )))
                .send()
                .await
        }
    });
    let mut busy = None;
    for _ in 0..200 {
        let answer = client
            .post(&url)
            .header("sec-fetch-site", "same-origin")
            .body("b")
            .send()
            .await
            .expect("sent");
        if answer.status().as_u16() == 503 {
            busy = Some(answer);
            break;
        }
    }
    let busy = busy.expect("a second body while the one slot is held");
    assert_carries_the_policy("a full streamed route", busy.headers());
    drop(held);
    assert_eq!(
        holding
            .await
            .expect("joined")
            .expect("answered")
            .status()
            .as_u16(),
        200
    );
    listener.stop().await.expect("stops");
}
