//! The page-serving contract the generated `register_pages` targets, over real HTTP.

#![cfg(not(target_arch = "wasm32"))]

use std::borrow::Cow;
use std::sync::Arc;

use fundamental_auth::{
    AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier, UserId,
};
use fundamental_core::AnyError;
use fundamental_core::i18n::{self, Arg, Locale, Translator, TranslatorSource};
use fundamental_core::secrets::SecretString;
use fundamental_web::prelude::*;
use fundamental_web::{
    CredentialSource, HttpContext, LocalePolicy, PageContext, PageData, Route, Router,
    ServerConfig, ServerError, ServerListener, WebServer, default_document, render_page,
    render_page_with,
};

#[derive(Clone)]
struct AppState {
    brand: &'static str,
}

fn home_page() -> impl IntoView {
    let state = use_context::<AppState>().expect("state in context");
    view! { <h1>"Welcome to "{state.brand}</h1> }
}

fn user_page() -> impl IntoView {
    let page = use_context::<PageContext>().expect("page context");
    let id = page.param("id").unwrap_or("?").to_owned();
    view! { <h1>"User "{id}</h1> }
}

fn account_page() -> impl IntoView {
    let page = use_context::<PageContext>().expect("page context");
    let who = page.principal().map_or_else(
        || "Sign in".to_owned(),
        |principal| format!("Signed in as {}", principal.user_id),
    );
    view! { <h1>{who}</h1> }
}

/// The surface's own answer to an address it does not serve, reached only when routing failed.
fn not_found_page() -> impl IntoView {
    let page = use_context::<PageContext>().expect("page context");
    page.set_response_header("X-Robots-Tag", "noindex");
    view! { <h1>"No such page"</h1> }
}

fn missing_page() -> impl IntoView {
    let page = use_context::<PageContext>().expect("page context");
    page.set_response_status(404);
    page.set_response_header("X-Robots-Tag", "noindex");
    view! { <h1>"Nothing here"</h1> }
}

/// A page whose text comes from the catalog, proving the translator was scoped around the render.
fn greeting_page() -> impl IntoView {
    let greeting = i18n::translate("greeting", &[], || Cow::Borrowed("Hello")).into_owned();
    view! { <h1>{greeting}</h1> }
}

#[derive(Clone)]
struct Devices {
    names: Vec<String>,
}

impl PageData<AppState> for Devices {
    async fn load(ctx: &HttpContext<AppState>) -> Result<Self, AnyError> {
        // The brand proves the loader saw the request's state rather than a captured constant.
        Ok(Self {
            names: vec![format!("{} laptop", ctx.state().brand)],
        })
    }
}

fn devices_page() -> impl IntoView {
    let devices = use_context::<Devices>().expect("the page's data is in context");
    view! { <ul>{devices.names.into_iter().map(|name| view! { <li>{name}</li> }).collect_view()}</ul> }
}

#[derive(Clone)]
struct Unavailable;

impl PageData<AppState> for Unavailable {
    async fn load(_ctx: &HttpContext<AppState>) -> Result<Self, AnyError> {
        Err(AnyError::new(ServerError::Internal(
            "the store did not answer".to_owned(),
        )))
    }
}

fn never_rendered_page() -> impl IntoView {
    view! { <h1>"this page must never be reached"</h1> }
}

fn layout(children: impl IntoView) -> impl IntoView {
    view! { <body><nav>"nav"</nav><main>{children}</main></body> }
}

fn document(ctx: &PageContext, body: &str) -> String {
    let lang = ctx.locale().map_or("en", Locale::language);
    format!("<!DOCTYPE html><html lang=\"{lang}\"><head><title>App</title></head>{body}</html>")
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn router() -> Router<AppState> {
    Router::new()
        .route(Route::html("/", |ctx: HttpContext<AppState>| async move {
            let rendered = render_page(&ctx, || layout(home_page()));
            Ok(document(rendered.context(), rendered.body()))
        }))
        .expect("unique route")
        .route(Route::html(
            "/users/{id}",
            |ctx: HttpContext<AppState>| async move {
                let rendered = render_page(&ctx, || layout(user_page()));
                Ok(default_document(rendered.context(), rendered.body()))
            },
        ))
        .expect("unique route")
        .route(Route::html(
            "/account",
            |ctx: HttpContext<AppState>| async move {
                let rendered = render_page(&ctx, || layout(account_page()));
                Ok(document(rendered.context(), rendered.body()))
            },
        ))
        .expect("unique route")
        .route(Route::html(
            "/missing",
            |ctx: HttpContext<AppState>| async move {
                let rendered = render_page(&ctx, || layout(missing_page()));
                Ok(document(rendered.context(), rendered.body()))
            },
        ))
        .expect("unique route")
        .route(Route::html(
            "/greeting",
            |ctx: HttpContext<AppState>| async move {
                let rendered = render_page(&ctx, || layout(greeting_page()));
                Ok(document(rendered.context(), rendered.body()))
            },
        ))
        .expect("unique route")
        .route(Route::html(
            "/devices",
            |ctx: HttpContext<AppState>| async move {
                let data = <Devices as PageData<AppState>>::load(&ctx).await?;
                let rendered = render_page_with(&ctx, data, || layout(devices_page()));
                Ok(document(rendered.context(), rendered.body()))
            },
        ))
        .expect("unique route")
        .route(Route::html(
            "/devices/unavailable",
            |ctx: HttpContext<AppState>| async move {
                let data = <Unavailable as PageData<AppState>>::load(&ctx).await?;
                let rendered = render_page_with(&ctx, data, || layout(never_rendered_page()));
                Ok(document(rendered.context(), rendered.body()))
            },
        ))
        .expect("unique route")
        .not_found(Route::html(
            "__not_found",
            |ctx: HttpContext<AppState>| async move {
                let rendered = render_page(&ctx, || layout(not_found_page()));
                Ok(document(rendered.context(), rendered.body()))
            },
        ))
}

fn config() -> ServerConfig {
    ServerConfig::new("127.0.0.1:0".parse().expect("addr"))
}

async fn start_with(config: ServerConfig) -> fundamental_web::WebListener {
    WebServer::new(config, AppState { brand: "Acme" }, router())
        .with_verifier(StubVerifier)
        .serve()
        .await
        .expect("binds an ephemeral port")
}

async fn start() -> fundamental_web::WebListener {
    start_with(config()).await
}

const SESSION: &str = "__Host-session";

struct StubVerifier;

impl PrincipalVerifier for StubVerifier {
    async fn verify(&self, token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        if token.expose() == "good-token" {
            Ok(AuthenticatedPrincipal {
                user_id: UserId::new("u1"),
                device_id: None,
                scopes: Vec::new(),
                expires_at: 0,
            })
        } else {
            Err(AuthError::Invalid(InvalidToken::Unrecognised))
        }
    }
}

struct DutchTranslator;

impl Translator for DutchTranslator {
    fn lookup(&self, key: &str, _args: &[Arg<'_>]) -> Option<String> {
        (key == "greeting").then(|| "Hallo".to_owned())
    }
}

struct DutchOnly;

impl TranslatorSource for DutchOnly {
    fn translator(&self, locale: &Locale) -> Arc<dyn Translator> {
        if locale.language() == "nl" {
            Arc::new(DutchTranslator)
        } else {
            Arc::new(i18n::NoopTranslator)
        }
    }
}

#[tokio::test]
async fn serves_a_page_through_its_layout_and_document() {
    let listener = start().await;
    let url = format!("http://{}/", listener.local_addr());

    let response = reqwest::get(&url).await.expect("request sent");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/html; charset=utf-8"),
    );
    let html = response.text().await.expect("text body");
    assert!(
        html.starts_with("<!DOCTYPE html><html lang=\"en\">"),
        "{html}"
    );
    assert!(html.contains("<title>App</title>"), "{html}");
    assert!(html.contains("<nav>nav</nav>"), "{html}");
    assert!(html.contains("Welcome to "), "{html}");
    assert!(html.contains("Acme"), "{html}");

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn serves_a_dynamic_page_reading_its_param() {
    let listener = start().await;
    let url = format!("http://{}/users/42", listener.local_addr());

    let response = reqwest::get(&url).await.expect("request sent");
    assert_eq!(response.status(), 200);
    let html = response.text().await.expect("text body");
    assert!(html.starts_with("<!DOCTYPE html>"), "{html}");
    assert!(html.contains("User "), "{html}");
    assert!(html.contains("42"), "{html}");

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_page_renders_signed_in_chrome_from_the_session_cookie() {
    let listener =
        start_with(config().with_credential_source(CredentialSource::cookie(SESSION))).await;
    let url = format!("http://{}/account", listener.local_addr());
    let client = reqwest::Client::new();

    let signed_in = client
        .get(&url)
        .header("cookie", format!("{SESSION}=good-token"))
        .send()
        .await
        .expect("request sent");
    assert_eq!(signed_in.status(), 200);
    let html = signed_in.text().await.expect("text body");
    assert!(html.contains("Signed in as u1"), "{html}");

    let signed_out = client.get(&url).send().await.expect("request sent");
    assert_eq!(
        signed_out.status(),
        200,
        "a public page must still serve a visitor with no session"
    );
    let html = signed_out.text().await.expect("text body");
    assert!(html.contains("Sign in"), "{html}");

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_page_answers_in_the_negotiated_language_shell_included() {
    i18n::set_translator_source(Arc::new(DutchOnly));
    let english: Locale = "en".parse().expect("a language tag");
    let dutch: Locale = "nl".parse().expect("a language tag");
    let policy = LocalePolicy::new(english).with_supported(vec![dutch]);

    let listener = start_with(config().with_locale_policy(policy)).await;
    let url = format!("http://{}/greeting", listener.local_addr());
    let client = reqwest::Client::new();

    let response = client
        .get(&url)
        .header("accept-language", "nl-NL,nl;q=0.9,en;q=0.4")
        .send()
        .await
        .expect("request sent");
    let html = response.text().await.expect("text body");
    assert!(html.contains("lang=\"nl\""), "{html}");
    assert!(html.contains("Hallo"), "{html}");

    let response = client
        .get(&url)
        .header("accept-language", "en")
        .send()
        .await
        .expect("request sent");
    let html = response.text().await.expect("text body");
    assert!(html.contains("lang=\"en\""), "{html}");
    assert!(
        html.contains("Hello"),
        "the source string, because nothing translates English: {html}"
    );

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_page_that_resolved_nothing_answers_404_with_its_body() {
    let listener = start().await;
    let url = format!("http://{}/missing", listener.local_addr());

    let response = reqwest::get(&url).await.expect("request sent");
    assert_eq!(response.status(), 404);
    assert_eq!(
        response
            .headers()
            .get("x-robots-tag")
            .and_then(|v| v.to_str().ok()),
        Some("noindex")
    );
    let html = response.text().await.expect("text body");
    assert!(
        html.contains("Nothing here"),
        "a human-facing 404 still renders its page: {html}"
    );

    listener.stop().await.expect("stops cleanly");
}

#[tokio::test]
async fn serves_a_page_whose_content_was_loaded_before_it_rendered() {
    let listener = start().await;
    let url = format!("http://{}/devices", listener.local_addr());

    let response = reqwest::get(&url).await.expect("request sent");
    assert_eq!(response.status(), 200);
    let body = response.text().await.expect("body");
    assert!(
        body.contains("<li>Acme laptop</li>"),
        "the loaded data is not in the first response: {body}"
    );
    // Loading data must not put a page on a different path through the shell.
    assert!(body.contains("<nav>nav</nav>"));
    assert!(body.starts_with("<!DOCTYPE html>"));
}

#[tokio::test]
async fn a_page_whose_data_cannot_be_loaded_is_not_rendered_at_all() {
    let listener = start().await;
    let url = format!("http://{}/devices/unavailable", listener.local_addr());

    let response = reqwest::get(&url).await.expect("request sent");
    assert_eq!(response.status(), 500);
    let body = response.text().await.expect("body");
    assert!(
        !body.contains("this page must never be reached"),
        "a page rendered around data that failed to load: {body}"
    );
}

#[tokio::test]
async fn an_unmatched_navigation_is_answered_by_the_surfaces_own_page() {
    let listener = start().await;
    let url = format!("http://{}/no/such/place", listener.local_addr());

    let response = reqwest::Client::new()
        .get(&url)
        .header("accept", "text/html,application/xhtml+xml")
        .send()
        .await
        .expect("request sent");

    assert_eq!(response.status(), 404, "the page answers 404 by default");
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/html; charset=utf-8"),
    );
    assert_eq!(
        response
            .headers()
            .get("x-robots-tag")
            .and_then(|v| v.to_str().ok()),
        Some("noindex"),
    );

    let html = response.text().await.expect("text body");
    assert!(html.starts_with("<!DOCTYPE html>"), "{html}");
    assert!(
        html.contains("<nav>nav</nav>"),
        "in the site chrome: {html}"
    );
    assert!(html.contains("No such page"), "{html}");
}

#[tokio::test]
async fn an_unmatched_api_request_still_gets_the_error_envelope() {
    let listener = start().await;
    let url = format!("http://{}/no/such/place", listener.local_addr());

    let response = reqwest::Client::new()
        .get(&url)
        .header("accept", "application/json")
        .send()
        .await
        .expect("request sent");

    assert_eq!(response.status(), 404);
    let body = response.text().await.expect("text body");
    assert!(body.starts_with('{'), "expected the envelope, got: {body}");
    assert!(!body.contains("No such page"), "{body}");
}
