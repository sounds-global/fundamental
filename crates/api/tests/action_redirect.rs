//! The redirect seam over the real HTTP backend.

use fundamental_api::{
    AnyError, ApiServer, HttpContext, Redirect, Router, ServerConfig, ServerError, ServerListener,
};
use fundamental_macros::action;
use serde::{Deserialize, Serialize};

// Referenced only inside the handlers' signatures, which the macro relocates.
type _Ctx = HttpContext<AppState>;

#[derive(Clone)]
struct AppState;

#[derive(Serialize, Deserialize)]
struct Credentials {
    code: String,
}

#[action(POST, "/login", form)]
async fn sign_in(ctx: HttpContext<AppState>, input: Credentials) -> Result<String, AnyError> {
    if input.code != "correct" {
        ctx.redirect(Redirect::see_other("/login?e=1")?);
        return Err(AnyError::new(ServerError::Unauthenticated(
            "the code was refused".to_owned(),
        )));
    }
    ctx.set_response_header("set-cookie", "__Host-session=t; Path=/; HttpOnly; Secure");
    ctx.redirect(Redirect::see_other("/welcome")?);
    Ok(String::new())
}

async fn start() -> fundamental_api::ApiListener {
    let router = Router::new().route(sign_in_route()).expect("unique route");
    ApiServer::new(
        ServerConfig::new("127.0.0.1:0".parse().expect("addr")),
        AppState,
        router,
    )
    .serve()
    .await
    .expect("binds an ephemeral port")
}

/// A client that does not follow redirects, so the `303` itself is observable.
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("a client")
}

#[tokio::test]
async fn an_accepted_form_answers_a_redirect_carrying_its_cookie() {
    let listener = start().await;
    let response = client()
        .post(format!("http://{}/login", listener.local_addr()))
        .form(&[("code", "correct")])
        .send()
        .await
        .expect("request sent");

    assert_eq!(response.status(), 303);
    assert_eq!(
        response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok()),
        Some("/welcome")
    );
    assert!(
        response
            .headers()
            .get("set-cookie")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("__Host-session=")),
        "the recorded Set-Cookie rides the redirect"
    );
    assert!(
        response.bytes().await.expect("a body").is_empty(),
        "a redirect carries no body"
    );
}

// To a browser posting a form with scripting off, a JSON envelope is the page.
#[tokio::test]
async fn a_refused_form_answers_a_redirect_rather_than_an_error_envelope() {
    let listener = start().await;
    let response = client()
        .post(format!("http://{}/login", listener.local_addr()))
        .form(&[("code", "wrong")])
        .send()
        .await
        .expect("request sent");

    assert_eq!(
        response.status(),
        303,
        "the refusal redirected instead of answering 401"
    );
    assert_eq!(
        response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok()),
        Some("/login?e=1")
    );
    let body = response.bytes().await.expect("a body");
    assert!(
        body.is_empty(),
        "no error envelope reaches a browser that asked for a page: {}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn a_handler_that_records_no_redirect_still_answers_its_error() {
    let listener = start().await;
    // No `code` field: the form fails to decode before the handler runs.
    let response = client()
        .post(format!("http://{}/login", listener.local_addr()))
        .form(&[("wrong_field", "x")])
        .send()
        .await
        .expect("request sent");

    assert_eq!(response.status(), 400, "the error's own status stands");
    assert!(
        !response.bytes().await.expect("a body").is_empty(),
        "and its envelope is served"
    );
}
