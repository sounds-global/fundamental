//! `#[action(..., accepts_form)]`: one endpoint for a JSON client and a browser form.

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
struct Signup {
    email: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Receipt {
    recorded: bool,
}

#[action(POST, "/waitlist", accepts_form)]
async fn join(ctx: HttpContext<AppState>, input: Signup) -> Result<Receipt, AnyError> {
    // The one branch is about the reply; both callers reach the validation and write alike.
    let from_a_browser_form = ctx.is_form_submission();

    if !input.email.contains('@') {
        if from_a_browser_form {
            // Still a typed error, so a JSON caller is not told the submission succeeded.
            ctx.redirect(Redirect::see_other("/join?invalid=1")?);
        }
        return Err(AnyError::new(ServerError::Unauthenticated(
            "not an address".to_owned(),
        )));
    }

    if from_a_browser_form {
        ctx.redirect(Redirect::see_other("/join?recorded=1")?);
    }
    Ok(Receipt { recorded: true })
}

async fn start() -> fundamental_api::ApiListener {
    let router = Router::new().route(join_route()).expect("unique route");
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
async fn a_json_client_gets_its_typed_receipt() {
    let listener = start().await;
    let url = format!("http://{}/waitlist", listener.local_addr());

    let response = no_redirects()
        .post(&url)
        .json(&Signup {
            email: "ada@example.test".to_owned(),
        })
        .send()
        .await
        .expect("sent");

    assert_eq!(response.status(), 200);
    let receipt: Receipt = response.json().await.expect("a typed receipt");
    assert!(receipt.recorded);

    listener.stop().await.expect("stops cleanly");
}

// `303` turns the follow-up into a `GET`, so a reload does not resubmit.
#[tokio::test]
async fn a_browser_form_post_is_recorded_and_redirected() {
    let listener = start().await;
    let url = format!("http://{}/waitlist", listener.local_addr());

    let response = no_redirects()
        .post(&url)
        .form(&[("email", "ada@example.test")])
        .send()
        .await
        .expect("sent");

    assert_eq!(
        response.status(),
        303,
        "a form post must be answered with a page the browser can navigate to, not JSON"
    );
    assert_eq!(
        response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok()),
        Some("/join?recorded=1")
    );
    assert!(
        response.text().await.expect("a body").is_empty(),
        "a redirect carries no content"
    );

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_content_type_carrying_a_charset_is_still_a_form() {
    let listener = start().await;
    let url = format!("http://{}/waitlist", listener.local_addr());

    for declared in [
        "application/x-www-form-urlencoded; charset=UTF-8",
        "Application/X-WWW-Form-Urlencoded",
    ] {
        let response = no_redirects()
            .post(&url)
            .header("content-type", declared)
            .body("email=ada%40example.test")
            .send()
            .await
            .expect("sent");
        assert_eq!(
            response.status(),
            303,
            "`{declared}` is the form media type and must decode as one"
        );
    }

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_refusal_answers_each_caller_in_its_own_shape() {
    let listener = start().await;
    let url = format!("http://{}/waitlist", listener.local_addr());
    let client = no_redirects();

    let from_a_form = client
        .post(&url)
        .form(&[("email", "not-an-address")])
        .send()
        .await
        .expect("sent");
    assert_eq!(from_a_form.status(), 303);
    assert_eq!(
        from_a_form
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok()),
        Some("/join?invalid=1"),
        "a reader who mistyped goes back to the form, not to an error document"
    );

    let from_json = client
        .post(&url)
        .json(&Signup {
            email: "not-an-address".to_owned(),
        })
        .send()
        .await
        .expect("sent");
    assert_eq!(
        from_json.status(),
        401,
        "a JSON caller still gets the typed refusal rather than a redirect it cannot follow"
    );

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_caller_that_declares_nothing_is_still_read_as_json() {
    let listener = start().await;
    let url = format!("http://{}/waitlist", listener.local_addr());

    let response = no_redirects()
        .post(&url)
        .body(r#"{"email":"ada@example.test"}"#)
        .send()
        .await
        .expect("sent");

    assert_eq!(response.status(), 200);
    listener.stop().await.expect("stops cleanly");
}
