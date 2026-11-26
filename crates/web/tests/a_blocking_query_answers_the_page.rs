//! A page's blocking query holds its response: the head, the status and the first chunk wait for it.
#![cfg(all(feature = "ssr", not(target_arch = "wasm32")))]

use fundamental_core::AnyError;
use fundamental_macros::{Error, action, mount_actions};
use fundamental_server::{HttpContext, Method, Route, ServerError, ServerRequest, ServerResponse};
use fundamental_web::prelude::*;
use fundamental_web::{PageContext, PageRoute, PolicyChain, Refused, Router, render_page};

#[action(GET, "/api/project", reactive(query))]
async fn project(_ctx: fundamental_server::HttpContext<()>) -> Result<String, AnyError> {
    tokio::task::yield_now().await;
    Ok("Held project".to_owned())
}

#[action(POST, "/api/named", reactive(query))]
async fn named(
    _ctx: fundamental_server::HttpContext<()>,
    name: String,
) -> Result<String, AnyError> {
    tokio::task::yield_now().await;
    Ok(format!("Project {name}"))
}

#[derive(Debug, Error)]
enum ProjectError {
    #[error(code = "PROJECT_403", message = "Not yours", status = 403)]
    NotYours,
}

#[action(GET, "/api/theirs", reactive(query))]
async fn theirs(_ctx: fundamental_server::HttpContext<()>) -> Result<String, AnyError> {
    Err(ProjectError::NotYours.into())
}

#[action(GET, "/api/members", reactive(query))]
async fn members(_ctx: fundamental_server::HttpContext<()>) -> Result<String, AnyError> {
    Err(ServerError::Unauthenticated("no session".to_owned()).into())
}

#[action(GET, "/api/broken", reactive(query))]
async fn broken(_ctx: fundamental_server::HttpContext<()>) -> Result<String, AnyError> {
    Err(ServerError::internal("the store is down").into())
}

mount_actions! { Project, Named, Theirs, Members, Broken }

#[component]
fn ProjectPage() -> impl IntoView {
    let project = actions::use_project().blocking();
    view! {
        <Query resource=project let:name>
            {expect_context::<PageContext>().set_title(name.clone())}
            <h1>{name}</h1>
        </Query>
    }
}

#[component]
fn NamedPage() -> impl IntoView {
    let named = actions::use_named().blocking().source(|| "Ada".to_owned());
    view! {
        <Query resource=named let:name>
            {expect_context::<PageContext>().set_title(name.clone())}
            <h1>{name}</h1>
        </Query>
    }
}

#[component]
fn TheirsPage() -> impl IntoView {
    let theirs = actions::use_theirs().blocking();
    view! { <Query resource=theirs let:name><h1>{name}</h1></Query> }
}

#[component]
fn MembersPage() -> impl IntoView {
    let members = actions::use_members().blocking();
    view! { <Query resource=members let:name><h1>{name}</h1></Query> }
}

#[component]
fn BrokenPage() -> impl IntoView {
    let broken = actions::use_broken().blocking();
    view! { <Query resource=broken let:name><h1>{name}</h1></Query> }
}

fn gate_page(refused: Refused) -> impl IntoView {
    view! { <h1>{format!("Gate {}", refused.status())}</h1> }
}

fn document(page: &PageContext, body: &str) -> String {
    let title = page.head().title.unwrap_or_default();
    format!("<!doctype html><html><head><title>{title}</title></head><body>{body}</body></html>")
}

fn router() -> Router<()> {
    register_actions(Router::new())
        .expect("actions register")
        .route(
            PageRoute::new("/project", PolicyChain::root())
                .expect("page policy")
                .view(ProjectPage, document),
        )
        .expect("page route registers")
        .route(
            PageRoute::new("/named", PolicyChain::root())
                .expect("page policy")
                .view(NamedPage, document),
        )
        .expect("page route registers")
        .route(
            PageRoute::new("/theirs", PolicyChain::root())
                .expect("page policy")
                .view(TheirsPage, document),
        )
        .expect("page route registers")
        .route(
            PageRoute::new("/members", PolicyChain::root())
                .expect("page policy")
                .view(MembersPage, document),
        )
        .expect("page route registers")
        .route(
            PageRoute::new("/broken", PolicyChain::root())
                .expect("page policy")
                .view(BrokenPage, document),
        )
        .expect("page route registers")
        .not_found(Route::html(
            "__not_found",
            |ctx: HttpContext<()>| async move {
                let rendered = render_page(&ctx, || view! { <h1>"Not found"</h1> });
                Ok(document(rendered.context(), rendered.body()))
            },
        ))
        .error_page(Route::html("__error", |ctx: HttpContext<()>| async move {
            let status = ctx.shown_error().map_or(0, |shown| shown.status());
            let rendered =
                render_page(&ctx, move || view! { <h1>{format!("Error {status}")}</h1> });
            Ok(document(rendered.context(), rendered.body()))
        }))
        .gate_page(PageRoute::gate(gate_page, document))
}

async fn navigate(router: &Router<()>, path: &str) -> (u16, String) {
    let request = ServerRequest::new(Method::Get, path, ()).with_headers(
        [
            ("sec-fetch-mode".to_owned(), "navigate".to_owned()),
            ("accept".to_owned(), "text/html".to_owned()),
        ]
        .into_iter()
        .collect(),
    );
    let response: ServerResponse = router.dispatch(request).await.buffered().await;
    let html = std::str::from_utf8(response.body())
        .expect("HTML is UTF-8")
        .to_owned();
    (response.status(), html)
}

/// The document as it reads before any streamed chunk replaces part of it.
fn shell(html: &str) -> &str {
    html.split_once("<template")
        .map_or(html, |(shell, _)| shell)
}

#[tokio::test]
async fn a_blocking_query_names_the_page_and_is_sent_without_its_fallback() {
    let (status, html) = navigate(&router(), "/project").await;

    assert_eq!(status, 200, "{html}");
    assert!(html.contains("<title>Held project</title>"), "{html}");
    assert!(shell(&html).contains("<h1>Held project</h1>"), "{html}");
    assert!(!html.contains("Loading"), "no pending fallback: {html}");
}

#[tokio::test]
async fn a_blocking_query_with_an_input_names_the_page() {
    let (status, html) = navigate(&router(), "/named").await;

    assert_eq!(status, 200, "{html}");
    assert!(html.contains("<title>Project Ada</title>"), "{html}");
    assert!(shell(&html).contains("<h1>Project Ada</h1>"), "{html}");
    assert!(!html.contains("Loading"), "no pending fallback: {html}");
}

#[tokio::test]
async fn a_blocking_query_refused_about_its_object_answers_the_not_found_page() {
    let (status, html) = navigate(&router(), "/theirs").await;

    assert_eq!(status, 404, "{html}");
    assert!(html.contains("<h1>Not found</h1>"), "{html}");
}

#[tokio::test]
async fn a_blocking_query_asking_for_sign_in_answers_the_gate() {
    let (status, html) = navigate(&router(), "/members").await;

    assert_eq!(status, 401, "{html}");
    assert!(html.contains("<h1>Gate 401</h1>"), "{html}");
}

#[tokio::test]
async fn a_blocking_query_that_fails_answers_the_error_page() {
    let (status, html) = navigate(&router(), "/broken").await;

    assert_eq!(status, 500, "{html}");
    assert!(html.contains("<h1>Error 500</h1>"), "{html}");
}
