//! On the SSR build, an action's route builder, browser stub and generated hook compile together.
#![cfg(all(feature = "ssr", not(target_arch = "wasm32")))]

use fundamental_core::AnyError;
use fundamental_core::action::Outcome;
use fundamental_core::address::SegmentText;
use fundamental_macros::{action, middleware, mount_actions};
use fundamental_server::{HttpContext, Method, Next, ServerRequest};
use fundamental_web::prelude::*;
use fundamental_web::{PageRoute, PolicyChain, Router};
use std::sync::atomic::{AtomicUsize, Ordering};

static STATS_CALLS: AtomicUsize = AtomicUsize::new(0);
static STATS_MIDDLEWARE_CALLS: AtomicUsize = AtomicUsize::new(0);

#[middleware]
async fn count_stats(
    _ctx: HttpContext<()>,
    body: Vec<u8>,
    next: Next<()>,
) -> fundamental_server::MiddlewareResult {
    STATS_MIDDLEWARE_CALLS.fetch_add(1, Ordering::SeqCst);
    next.run(_ctx, body).await
}

#[action(POST, "/greet")]
async fn greet(ctx: HttpContext<()>, name: String) -> Result<Outcome<String>, AnyError> {
    let _ = ctx;
    Ok(Outcome::changed(format!("hello, {name}")).said("Greeted."))
}

#[action(GET, "/stats", count_stats)]
async fn stats(_ctx: HttpContext<()>) -> Result<String, AnyError> {
    STATS_CALLS.fetch_add(1, Ordering::SeqCst);
    Ok("ready".to_owned())
}

#[action(POST, "/lookup", reactive(query))]
async fn lookup(_ctx: HttpContext<()>, name: String) -> Result<String, AnyError> {
    Ok(format!("found {name}"))
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct SearchFilter {
    term: String,
}

#[action(GET, "/search")]
async fn search(_ctx: HttpContext<()>, filter: SearchFilter) -> Result<String, AnyError> {
    Ok(format!("searched {}", filter.term))
}

#[action(GET, "/teams/{team}/roster")]
async fn roster(ctx: HttpContext<()>) -> Result<String, AnyError> {
    Ok(format!(
        "roster of {}",
        ctx.params().get("team").unwrap_or("?")
    ))
}

#[action(GET, "/teams/{team}/search")]
async fn team_search(ctx: HttpContext<()>, filter: SearchFilter) -> Result<String, AnyError> {
    let team = ctx.params().get("team").unwrap_or("?").to_owned();
    Ok(format!("{team} searched {}", filter.term))
}

mount_actions! { Greet, Stats, Lookup, Search, Roster, TeamSearch }

fn acme() -> SegmentText {
    SegmentText::new("acme").expect("a segment")
}

#[test]
fn the_route_registers_and_the_browser_symbols_exist_on_the_ssr_build() {
    let _router: Router<()> = Router::new().route(greet_route()).expect("unique route");

    // An island renders on the server before hydrating, so these must compile there.
    let _stub = greet;
    let _hook = actions::use_greet;
}

#[component]
fn generated_hook_is_composable_with_mutation() -> impl IntoView {
    let greet = actions::use_greet();
    view! {
        <Mutation mutation=greet let:mutation>
            <button on:click=move |_| mutation.submit("Ada".to_owned())>
                "Greet"
            </button>
        </Mutation>
    }
}

#[island]
fn ExampleQueryPage() -> impl IntoView {
    let stats = actions::use_stats();
    let lookup = actions::use_lookup().source(|| "Ada".to_owned());
    view! {
        <Query resource=stats let:value>
            <p>{value}</p>
        </Query>
        <Query resource=lookup let:value>
            <p>{value}</p>
        </Query>
    }
}

#[tokio::test]
async fn a_generated_action_query_runs_on_the_server_and_is_serialized_for_hydration() {
    STATS_CALLS.store(0, Ordering::SeqCst);
    STATS_MIDDLEWARE_CALLS.store(0, Ordering::SeqCst);
    let document = |_: &fundamental_web::PageContext, body: &str| {
        format!("<!doctype html><html><body>{body}</body></html>")
    };
    let router = register_actions(Router::new())
        .expect("actions register")
        .route(
            PageRoute::new("/example", PolicyChain::root())
                .expect("page policy")
                .view(ExampleQueryPage, document),
        )
        .expect("page route registers");
    let response = router
        .dispatch(ServerRequest::new(Method::Get, "/example", ()))
        .await
        .buffered()
        .await;
    let html = std::str::from_utf8(response.body()).expect("HTML is UTF-8");

    assert_eq!(STATS_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(STATS_MIDDLEWARE_CALLS.load(Ordering::SeqCst), 1);
    assert!(
        html.contains("ready"),
        "server query result missing: {html}"
    );
    assert!(
        html.contains("found Ada"),
        "input query result missing: {html}"
    );
    assert!(
        html.contains("__RESOLVED_RESOURCES=["),
        "resource not serialized: {html}"
    );
}

#[island]
fn SearchPage() -> impl IntoView {
    let results = actions::use_search().source(|| SearchFilter {
        term: "Ada Lovelace".to_owned(),
    });
    view! {
        <Query resource=results let:value>
            <p>{value}</p>
        </Query>
    }
}

async fn render(page: impl Fn() -> AnyView + Clone + Send + Sync + 'static) -> String {
    let document = |_: &fundamental_web::PageContext, body: &str| {
        format!("<!doctype html><html><body>{body}</body></html>")
    };
    let router = register_actions(Router::new())
        .expect("actions register")
        .route(
            PageRoute::new("/page", PolicyChain::root())
                .expect("page policy")
                .view(page, document),
        )
        .expect("page route registers");
    let response = router
        .dispatch(ServerRequest::new(Method::Get, "/page", ()))
        .await
        .buffered()
        .await;
    String::from_utf8(response.body().to_vec()).expect("HTML is UTF-8")
}

#[tokio::test]
async fn a_get_query_sends_its_source_as_the_query_string_on_the_server() {
    let html = render(|| view! { <SearchPage/> }.into_any()).await;
    assert!(
        html.contains("searched Ada Lovelace"),
        "the filter did not reach the route: {html}"
    );
}

#[island]
fn RosterPage() -> impl IntoView {
    let roster = actions::use_roster().at(|| Roster::path(acme()));
    let found = actions::use_team_search()
        .source(|| SearchFilter {
            term: "Ada".to_owned(),
        })
        .at(|| TeamSearch::path(acme()));
    view! {
        <Query resource=roster let:value>
            <p>{value}</p>
        </Query>
        <Query resource=found let:value>
            <p>{value}</p>
        </Query>
    }
}

#[tokio::test]
async fn a_query_at_a_filled_path_runs_with_its_captures_on_the_server() {
    let html = render(|| view! { <RosterPage/> }.into_any()).await;
    assert!(
        html.contains("roster of acme"),
        "the captures were lost: {html}"
    );
    assert!(
        html.contains("acme searched Ada"),
        "the captures or the filter were lost: {html}"
    );
}

#[component]
fn a_mutation_takes_a_filled_path() -> impl IntoView {
    let greet = actions::use_greet().at(Greet::path);
    view! {
        <Mutation mutation=greet let:mutation>
            <button on:click=move |_| mutation.submit("Ada".to_owned())>"Greet"</button>
        </Mutation>
    }
}

#[component]
fn a_filled_path_combines_with_every_hook_option_in_any_order() -> impl IntoView {
    let filter = || SearchFilter {
        term: "Ada".to_owned(),
    };
    let held = actions::use_team_search()
        .at(|| TeamSearch::path(acme()))
        .source(filter)
        .blocking();
    let local = actions::use_roster().local().at(|| Roster::path(acme()));
    let local_filtered = actions::use_team_search()
        .at(|| TeamSearch::path(acme()))
        .local()
        .source(filter);
    view! {
        <Query resource=held let:value><p>{value}</p></Query>
        <Query resource=local let:value><p>{value}</p></Query>
        <Query resource=local_filtered let:value><p>{value}</p></Query>
    }
}

#[island]
fn UnfilledRosterPage() -> impl IntoView {
    let roster = actions::use_roster();
    view! {
        <Query resource=roster let:value>
            <p>{value}</p>
        </Query>
    }
}

#[tokio::test]
async fn a_query_whose_path_holds_captures_is_refused_without_at() {
    let html = render(|| view! { <UnfilledRosterPage/> }.into_any()).await;
    assert!(
        !html.contains("roster of"),
        "the template reached the route: {html}"
    );
    assert!(
        html.contains("The request could not be completed."),
        "the refusal is not shown: {html}"
    );
}
