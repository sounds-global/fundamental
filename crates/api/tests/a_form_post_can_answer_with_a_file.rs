//! A browser's own form post answered with a file.

use fundamental_api::{
    AnyError, ApiServer, HttpContext, Redirect, Router, ServerConfig, ServerError, ServerListener,
};
use fundamental_macros::action;
use serde::{Deserialize, Serialize};

// Referenced only inside the handler signature, which the macro relocates.
type _Ctx = HttpContext<AppState>;

#[derive(Clone)]
struct AppState;

#[derive(Serialize, Deserialize)]
struct ExportRequest {
    understood: Option<String>,
}

#[action(
    POST,
    "/export",
    accepts_form,
    content_type = "application/json",
    bytes
)]
async fn export(ctx: HttpContext<AppState>, input: ExportRequest) -> Result<Vec<u8>, AnyError> {
    if input.understood.as_deref() != Some("on") {
        if ctx.is_form_submission() {
            ctx.redirect(Redirect::see_other("/account?refused=1")?);
        }
        return Err(AnyError::new(ServerError::Unauthenticated(
            "the box was not ticked".to_owned(),
        )));
    }
    ctx.set_response_header(
        "content-disposition",
        "attachment; filename=\"account.json\"",
    );
    Ok(br#"{"account":"a-1"}"#.to_vec())
}

async fn start() -> fundamental_api::ApiListener {
    let router = Router::new().route(export_route()).expect("unique route");
    ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("addr")),
        AppState,
        router,
    )
    .serve()
    .await
    .expect("binds an ephemeral port")
}

fn no_redirects() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("a client")
}

#[tokio::test]
async fn a_form_post_is_answered_with_the_file() {
    let listener = start().await;
    let url = format!("http://{}/export", listener.local_addr());

    let response = no_redirects()
        .post(&url)
        .form(&[("understood", "on")])
        .send()
        .await
        .expect("sent");

    assert_eq!(response.status(), 200);
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    assert_eq!(header("content-type").as_deref(), Some("application/json"));
    assert_eq!(
        header("content-disposition").as_deref(),
        Some("attachment; filename=\"account.json\"")
    );
    assert_eq!(
        response.bytes().await.expect("a body").as_ref(),
        br#"{"account":"a-1"}"#,
        "the bytes the handler produced, verbatim"
    );

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_json_client_gets_the_same_file() {
    let listener = start().await;
    let url = format!("http://{}/export", listener.local_addr());

    let response = no_redirects()
        .post(&url)
        .json(&ExportRequest {
            understood: Some("on".to_owned()),
        })
        .send()
        .await
        .expect("sent");

    assert_eq!(response.status(), 200);
    assert_eq!(
        response.bytes().await.expect("a body").as_ref(),
        br#"{"account":"a-1"}"#
    );

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_refused_form_post_goes_back_to_its_page() {
    let listener = start().await;
    let url = format!("http://{}/export", listener.local_addr());
    let client = no_redirects();

    let from_a_form = client
        .post(&url)
        .form::<[(&str, &str); 0]>(&[])
        .send()
        .await
        .expect("sent");
    assert_eq!(from_a_form.status(), 303);
    assert_eq!(
        from_a_form
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok()),
        Some("/account?refused=1")
    );
    assert!(
        from_a_form.text().await.expect("a body").is_empty(),
        "a redirect carries no file"
    );

    let from_json = client
        .post(&url)
        .json(&ExportRequest { understood: None })
        .send()
        .await
        .expect("sent");
    assert_eq!(
        from_json.status(),
        401,
        "a JSON caller gets the typed refusal"
    );

    listener.stop().await.expect("stops cleanly");
}
