//! Query-bound and content-typed `#[action]` forms end to end.

use fundamental_api::{AnyError, ApiServer, HttpContext, Router, ServerConfig, ServerListener};
use fundamental_macros::action;

// Referenced only inside the handlers' signatures, which the macro relocates.
type _Ctx = HttpContext<AppState>;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState;

#[derive(Serialize, Deserialize)]
struct ResolveQuery {
    sub: String,
    anchor: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Resolved {
    sub: String,
    anchor: String,
}

#[action(GET, "/resolve")]
async fn resolve(_ctx: HttpContext<AppState>, query: ResolveQuery) -> Result<Resolved, AnyError> {
    Ok(Resolved {
        sub: query.sub,
        anchor: query.anchor.unwrap_or_else(|| "default".to_owned()),
    })
}

#[action(GET, "/fetch", content_type = "application/jwt")]
async fn fetch(_ctx: HttpContext<AppState>, query: ResolveQuery) -> Result<String, AnyError> {
    Ok(format!("signed-statement-for-{}", query.sub))
}

async fn start(router: Router<AppState>) -> fundamental_api::ApiListener {
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
async fn get_action_binds_its_input_from_the_query_string() {
    let router = Router::new().route(resolve_route()).expect("unique route");
    let listener = start(router).await;
    let url = format!(
        "http://{}/resolve?sub=https://issuer.example&anchor=https://root.example",
        listener.local_addr()
    );

    let response = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .expect("request sent");
    assert_eq!(response.status(), 200);
    let out: Resolved = response.json().await.expect("json body");
    assert_eq!(out.sub, "https://issuer.example");
    assert_eq!(out.anchor, "https://root.example");

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn missing_required_query_field_is_a_400() {
    let router = Router::new().route(resolve_route()).expect("unique route");
    let listener = start(router).await;
    // `sub` is required; omit it.
    let url = format!("http://{}/resolve?anchor=x", listener.local_addr());

    let response = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .expect("request sent");
    assert_eq!(response.status(), 400);

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn content_type_action_returns_a_raw_body_under_its_media_type() {
    let router = Router::new().route(fetch_route()).expect("unique route");
    let listener = start(router).await;
    let url = format!(
        "http://{}/fetch?sub=https://issuer.example",
        listener.local_addr()
    );

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
        Some("application/jwt")
    );
    let body = response.text().await.expect("text body");
    assert_eq!(body, "signed-statement-for-https://issuer.example");

    listener.stop().await.expect("stops cleanly");
}
