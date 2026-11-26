//! An HTML route's `text/html` content type survives the axum backend.

use fundamental_api::{
    AnyError, ApiServer, HttpContext, Route, Router, ServerConfig, ServerListener,
};

#[derive(Clone)]
struct AppState {
    title: &'static str,
}

async fn start(router: Router<AppState>) -> fundamental_api::ApiListener {
    ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("addr")),
        AppState { title: "Home" },
        router,
    )
    .serve()
    .await
    .expect("binds an ephemeral port")
}

#[tokio::test]
async fn html_route_serves_text_html_over_http() {
    let router = Router::new()
        .route(Route::html(
            "/page",
            |ctx: HttpContext<AppState>| async move {
                Ok::<_, AnyError>(format!("<h1>{}</h1>", ctx.state().title))
            },
        ))
        .expect("unique route");
    let listener = start(router).await;
    let url = format!("http://{}/page", listener.local_addr());

    let response = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .expect("request sent");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/html; charset=utf-8"),
    );
    let body = response.text().await.expect("text body");
    assert_eq!(body, "<h1>Home</h1>");

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn json_route_still_serves_application_json() {
    let router = Router::new()
        .route(Route::get(
            "/data",
            |_ctx: HttpContext<AppState>, _input: ()| async move {
                Ok::<_, AnyError>(serde_json::json!({ "ok": true }))
            },
        ))
        .expect("unique route");
    let listener = start(router).await;
    let url = format!("http://{}/data", listener.local_addr());

    let response = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .expect("request sent");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/json"),
    );

    listener.stop().await.expect("stops cleanly");
}
