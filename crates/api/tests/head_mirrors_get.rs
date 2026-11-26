//! `HEAD` answers wherever `GET` does, over real HTTP; the router never sees a `HEAD`.

use fundamental_api::{
    AnyError, ApiServer, HttpContext, Route, Router, ServerConfig, ServerListener,
};

#[derive(Clone)]
struct AppState;

const PAGE: &str = "<h1>Acme</h1>";

async fn start() -> fundamental_api::ApiListener {
    let router = Router::new()
        .route(Route::html(
            "/page",
            |_: HttpContext<AppState>| async move { Ok::<_, AnyError>(PAGE.to_owned()) },
        ))
        .expect("unique route")
        .route(Route::post(
            "/submit",
            |_: HttpContext<AppState>, (): ()| async move { Ok::<_, AnyError>(()) },
        ))
        .expect("unique route");
    ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("addr")),
        AppState,
        router,
    )
    .serve()
    .await
    .expect("binds an ephemeral port")
}

#[tokio::test]
async fn head_answers_where_get_answers() {
    let listener = start().await;
    let url = format!("http://{}/page", listener.local_addr());

    let response = reqwest::Client::new()
        .head(&url)
        .send()
        .await
        .expect("request sent");

    assert_eq!(
        response.status(),
        200,
        "a resource that serves GET must serve HEAD"
    );
    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn head_carries_the_headers_get_would_have_sent_and_no_body() {
    let listener = start().await;
    let url = format!("http://{}/page", listener.local_addr());
    let client = reqwest::Client::new();

    let got = client.get(&url).send().await.expect("GET sent");
    let content_type = got
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = got.text().await.expect("a body");

    let headed = client.head(&url).send().await.expect("HEAD sent");
    assert_eq!(
        headed
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
        content_type,
        "HEAD must describe the same representation GET would have returned"
    );
    assert_eq!(
        headed
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok()),
        Some(body.len()),
        "Content-Length must be what GET would have sent, not the empty body's 0"
    );
    assert!(
        headed.text().await.expect("a body").is_empty(),
        "a HEAD response carries no content"
    );

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn head_is_refused_exactly_where_get_is_refused() {
    let listener = start().await;
    let base = listener.local_addr().address().expect("a bound address");
    let client = reqwest::Client::new();

    let url = format!("http://{base}/submit");
    assert_eq!(
        client.head(&url).send().await.expect("sent").status(),
        client.get(&url).send().await.expect("sent").status(),
        "HEAD and GET must agree about a method-mismatched path"
    );

    let missing = format!("http://{base}/nowhere");
    assert_eq!(
        client.head(&missing).send().await.expect("sent").status(),
        404,
        "HEAD on a path nothing serves is a 404, not a 405"
    );

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn the_refusal_names_head_among_the_allowed_methods() {
    let listener = start().await;
    let url = format!("http://{}/page", listener.local_addr());

    let response = reqwest::Client::new()
        .delete(&url)
        .send()
        .await
        .expect("request sent");
    assert_eq!(response.status(), 405);
    let allow = response
        .headers()
        .get("allow")
        .and_then(|value| value.to_str().ok())
        .expect("RFC 9110 §15.5.6 requires Allow on a 405")
        .to_owned();
    assert!(allow.contains("GET"), "{allow}");
    assert!(allow.contains("HEAD"), "{allow}");

    listener.stop().await.expect("stops cleanly");
}
