//! A page's in-process call to a mounted action, at its own path, its segments resolved before its guard.

use fundamental_core::AnyError;
use fundamental_core::action::Action;
use fundamental_core::i18n::Locale;
use fundamental_server::{
    HttpContext, Method, Middleware, MiddlewareResult, Next, ResolvedSegment, Route, Router,
    SegmentResolution, SegmentResolver, ServerAction, ServerError, ServerRequest, ServerResponse,
};
use serde::Deserialize;

#[derive(Clone)]
struct State;

struct Members;

impl Action for Members {
    type Input = ();
    type Output = String;
    const METHOD: &'static str = "GET";
    const PATH: &'static str = "/teams/{team}/members";
}

impl ServerAction<State> for Members {
    fn route() -> Route<State> {
        Route::action(
            Method::Get,
            Self::PATH,
            |ctx: HttpContext<State>, (): ()| async move {
                Ok(format!(
                    "members of {}",
                    ctx.params().get("team").unwrap_or("?")
                ))
            },
        )
    }
}

struct TeamHandles;

impl SegmentResolver for TeamHandles {
    fn param(&self) -> &str {
        "team"
    }

    fn void_id(&self) -> String {
        "void".to_owned()
    }

    async fn resolve<'a>(
        &'a self,
        raw: &'a str,
        _scope: Option<&'a ResolvedSegment>,
        _locale: Option<&'a Locale>,
    ) -> Result<SegmentResolution, AnyError> {
        Ok(match raw {
            "acme" => SegmentResolution::Canonical(
                ResolvedSegment::new("t-acme", "acme").expect("a valid spelling"),
            ),
            _ => SegmentResolution::Unknown,
        })
    }
}

struct MemberOfAcme;

impl Middleware<State> for MemberOfAcme {
    async fn handle(
        &self,
        ctx: HttpContext<State>,
        body: Vec<u8>,
        next: Next<'_, State>,
    ) -> MiddlewareResult {
        if ctx.params().get("team") != Some("t-acme") {
            return Err(AnyError::new(ServerError::Unauthenticated(
                "not a member".to_owned(),
            )));
        }
        next.run(ctx, body).await
    }
}

struct Roster;

impl Action for Roster {
    type Input = ();
    type Output = String;
    const METHOD: &'static str = "GET";
    const PATH: &'static str = "/teams/{team}/roster";
}

impl ServerAction<State> for Roster {
    fn route() -> Route<State> {
        Route::action(
            Method::Get,
            Self::PATH,
            |ctx: HttpContext<State>, (): ()| async move {
                Ok(format!(
                    "roster of {}",
                    ctx.params().get("team").unwrap_or("?")
                ))
            },
        )
        .middleware(MemberOfAcme)
    }
}

#[derive(Deserialize)]
struct SessionFilter {
    active: bool,
}

struct Sessions;

impl Action for Sessions {
    type Input = SessionFilter;
    type Output = String;
    const METHOD: &'static str = "GET";
    const PATH: &'static str = "/teams/{team}/sessions";
}

impl ServerAction<State> for Sessions {
    fn route() -> Route<State> {
        Route::action(
            Method::Get,
            Self::PATH,
            |ctx: HttpContext<State>, filter: SessionFilter| async move {
                let team = ctx.params().get("team").unwrap_or("?").to_owned();
                Ok(format!("{team} active={}", filter.active))
            },
        )
    }
}

/// A page at `/pages/{page}` that renders what dispatching `M` at `target` answers.
fn router<M: 'static>(target: &'static str) -> Router<State> {
    Router::new()
        .action::<Members>()
        .and_then(Router::action::<Sessions>)
        .and_then(Router::action::<Roster>)
        .and_then(|router| router.segment(TeamHandles))
        .and_then(|router| {
            router.route(Route::html(
                "/pages/{page}",
                move |ctx: HttpContext<State>| async move {
                    let dispatcher = ctx.action_dispatcher().expect("a mounted dispatcher");
                    let answer = dispatcher.dispatch_at::<M>(target, Vec::new()).await;
                    Ok(match answer {
                        Ok(body) => String::from_utf8(body).expect("utf-8"),
                        Err(error) => format!("refused: {}", error.get().error_code()),
                    })
                },
            ))
        })
        .expect("a well-declared router")
}

async fn page(router: &Router<State>) -> String {
    let response: ServerResponse = router
        .dispatch(ServerRequest::new(Method::Get, "/pages/home", State))
        .await;
    assert_eq!(response.status(), 200);
    String::from_utf8(response.body().to_vec()).expect("utf-8")
}

#[tokio::test]
async fn a_dispatched_action_sees_its_own_captures_not_the_pages() {
    let body = page(&router::<Members>("/teams/acme/members")).await;
    assert_eq!(body, "\"members of t-acme\"");
}

#[tokio::test]
async fn a_path_that_is_not_the_actions_is_refused() {
    let body = page(&router::<Members>("/teams/acme/other")).await;
    assert_eq!(body, "refused: API_034");
}

#[tokio::test]
async fn a_dispatched_get_action_reads_its_input_from_the_query() {
    let body = page(&router::<Sessions>("/teams/acme/sessions?active=true")).await;
    assert_eq!(body, "\"t-acme active=true\"");
}

#[tokio::test]
async fn a_declared_segment_is_resolved_before_the_actions_guard() {
    let body = page(&router::<Roster>("/teams/acme/roster")).await;
    assert_eq!(body, "\"roster of t-acme\"");
}
