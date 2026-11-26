//! A navigation refused by the backend itself gets the error page; a program keeps the envelope.

use std::time::Duration;

use fundamental_api::{ApiListener, ApiServer};
use fundamental_core::AnyError;
use fundamental_macros::Error;
use fundamental_server::{
    Admission, AdmissionRequest, HttpContext, Route, Router, ServerConfig, ServerListener,
};
use reqwest::header::{CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER, VARY};

#[derive(Debug, Error)]
enum Refused {
    #[error(
        code = "TEST_429",
        message = "Refused at admission",
        developer_message = "budget for 10.0.0.0/24 spent",
        status = 429,
        retryable,
        retry_after = retry_after_seconds
    )]
    Budget { retry_after_seconds: u64 },
}

#[derive(Debug)]
struct Refusing;

impl Admission for Refusing {
    fn admit(&self, _request: &AdmissionRequest<'_>) -> Result<(), AnyError> {
        Err(Refused::Budget {
            retry_after_seconds: 17,
        }
        .into())
    }
}

async fn error_page(context: HttpContext<()>) -> Result<String, AnyError> {
    let shown = context.shown_error();
    Ok(format!(
        "<h1>{}</h1>",
        shown.map_or("nothing".to_owned(), |shown| format!(
            "{} {} {:?}",
            shown.code(),
            shown.status(),
            shown.retry_after_secs()
        ))
    ))
}

async fn failing(_context: HttpContext<()>) -> Result<String, AnyError> {
    Err(Refused::Budget {
        retry_after_seconds: 3,
    }
    .into())
}

async fn slow(_context: HttpContext<()>) -> Result<String, AnyError> {
    tokio::time::sleep(Duration::from_secs(5)).await;
    Ok("too late".to_owned())
}

async fn serve(config: ServerConfig) -> ApiListener {
    let router = Router::<()>::new()
        .route(Route::html("/slow", slow))
        .expect("the route registers")
        .route(Route::html("/failing", failing))
        .expect("the route registers")
        .error_page(Route::html("__error", error_page));
    ApiServer::new(config, (), router)
        .serve()
        .await
        .expect("binds an ephemeral port")
}

fn bind() -> ServerConfig {
    ServerConfig::new("127.0.0.1:0".parse().expect("an ephemeral bind"))
}

async fn get(listener: &ApiListener, path: &str, mode: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("http://{}{path}", listener.local_addr()))
        .header("sec-fetch-mode", mode)
        .header("accept", "text/html,*/*;q=0.8")
        .send()
        .await
        .expect("the server answers")
}

fn header(response: &reqwest::Response, name: reqwest::header::HeaderName) -> String {
    response
        .headers()
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join(", ")
}

#[tokio::test]
async fn a_navigation_refused_at_admission_gets_the_error_page_with_retry_after() {
    let listener = serve(bind().with_admission(Refusing)).await;
    let response = get(&listener, "/anything", "navigate").await;
    assert_eq!(response.status().as_u16(), 429);
    assert_eq!(header(&response, RETRY_AFTER), "17");
    assert_eq!(header(&response, CONTENT_TYPE), "text/html; charset=utf-8");
    assert_eq!(header(&response, CACHE_CONTROL), "no-store");
    assert!(header(&response, VARY).contains("Sec-Fetch-Mode, Accept"));
    let body = response.text().await.expect("a body");
    assert_eq!(body, "<h1>TEST_429 429 Some(17)</h1>");
}

#[tokio::test]
async fn a_fetch_refused_at_admission_gets_the_envelope() {
    let listener = serve(bind().with_admission(Refusing)).await;
    let response = get(&listener, "/anything", "cors").await;
    assert_eq!(response.status().as_u16(), 429);
    assert_eq!(header(&response, RETRY_AFTER), "17");
    assert_eq!(header(&response, CONTENT_TYPE), "application/json");
    assert!(header(&response, VARY).contains("Sec-Fetch-Mode, Accept"));
    let body = response.text().await.expect("a body");
    assert!(body.starts_with("{\"error\":\"TEST_429\""), "{body}");
    assert!(
        !body.contains("10.0.0.0"),
        "never the developer message: {body}"
    );
}

#[tokio::test]
async fn a_navigation_stopped_at_its_deadline_gets_the_error_page() {
    let listener = serve(bind().with_request_timeout(Duration::from_millis(200))).await;
    let response = get(&listener, "/slow", "navigate").await;
    assert_eq!(response.status().as_u16(), 408);
    let body = response.text().await.expect("a body");
    assert_eq!(body, "<h1>API_021 408 None</h1>");
}

#[tokio::test]
async fn a_routes_error_to_a_program_varies_once_and_keeps_the_envelope() {
    let listener = serve(bind()).await;
    let response = get(&listener, "/failing", "cors").await;
    assert_eq!(response.status().as_u16(), 429);
    let varies: Vec<_> = response
        .headers()
        .get_all(VARY)
        .iter()
        .filter(|value| value.to_str().is_ok_and(|v| v.contains("Sec-Fetch-Mode")))
        .collect();
    assert_eq!(
        varies.len(),
        1,
        "one Vary for the representation: {varies:?}"
    );
    assert!(header(&response, CONTENT_TYPE).starts_with("application/json"));
}
