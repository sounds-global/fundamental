//! Declared path segments through a router: resolved before the guard, one `301` only once affirmed.

use fundamental_core::AnyError;
use fundamental_core::i18n::Locale;
use fundamental_server::{
    HttpContext, Method, Middleware, MiddlewareResult, Next, ResolvedSegment, Route, Router,
    SegmentResolution, SegmentResolver, ServerError, ServerRequest, ServerResponse,
};

#[derive(Clone)]
struct State;

/// Team slugs: `sounds-global` is current, `old-name` its slug before a rename, `rival` another team's.
struct TeamSlugs;

impl SegmentResolver for TeamSlugs {
    fn param(&self) -> &str {
        "team"
    }

    fn namespace(&self) -> Option<&str> {
        Some("team")
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
            "sounds-global" => SegmentResolution::Canonical(segment("t-a", "sounds-global")),
            "old-name" => SegmentResolution::Moved(segment("t-a", "sounds-global")),
            "rival" => SegmentResolution::Canonical(segment("t-b", "rival")),
            "rival-old" => SegmentResolution::Moved(segment("t-b", "rival")),
            "outage" => return Err(AnyError::new(ServerError::StreamBusy)),
            _ => SegmentResolution::Unknown,
        })
    }
}

/// Aanvraag numbers, looked up only inside the team they were resolved under.
struct BuildNumbers;

impl SegmentResolver for BuildNumbers {
    fn param(&self) -> &str {
        "build"
    }

    fn scope(&self) -> Option<&str> {
        Some("team")
    }

    fn void_id(&self) -> String {
        "void".to_owned()
    }

    async fn resolve<'a>(
        &'a self,
        raw: &'a str,
        scope: Option<&'a ResolvedSegment>,
        _locale: Option<&'a Locale>,
    ) -> Result<SegmentResolution, AnyError> {
        let team = scope.map(ResolvedSegment::id);
        Ok(match (team, raw) {
            (Some("t-a"), "12") => SegmentResolution::Canonical(segment("b-12", "12")),
            (Some("t-a"), "012") => SegmentResolution::Moved(segment("b-12", "12")),
            (Some("t-b"), "1") => SegmentResolution::Canonical(segment("b-1", "1")),
            (Some("t-b"), "01") => SegmentResolution::Moved(segment("b-1", "1")),
            _ => SegmentResolution::Unknown,
        })
    }
}

fn segment(id: &str, canonical: &str) -> ResolvedSegment {
    ResolvedSegment::new(id, canonical).expect("a valid canonical spelling")
}

/// A member of team `t-a` only, recording what it allowed as an authorization guard does.
struct MemberOfTeamA;

impl Middleware<State> for MemberOfTeamA {
    async fn handle(
        &self,
        ctx: HttpContext<State>,
        body: Vec<u8>,
        next: Next<'_, State>,
    ) -> MiddlewareResult {
        let team = ctx.params().get("team").unwrap_or_default().to_owned();
        if team != "t-a" {
            return Err(AnyError::new(ServerError::Unauthenticated(
                "not a member".to_owned(),
            )));
        }
        ctx.authorized_objects().record("team", "member", &team);
        next.run(ctx, body).await
    }
}

fn page(ctx: &HttpContext<State>) -> String {
    let team = ctx.params().get("team").unwrap_or_default();
    let build = ctx.params().get("build").unwrap_or_default();
    format!("{team}:{build}")
}

fn router() -> Router<State> {
    Router::new()
        .segment(TeamSlugs)
        .and_then(|router| router.segment(BuildNumbers))
        .and_then(|router| {
            router.route(
                Route::html(
                    "/{team}/aanvragen/{build}",
                    |ctx: HttpContext<State>| async move {
                        // The build is the team's, so the team's guard covers it.
                        ctx.affirm("build");
                        Ok(page(&ctx))
                    },
                )
                .middleware(MemberOfTeamA),
            )
        })
        .and_then(|router| {
            router.route(
                Route::html("/{team}", |ctx: HttpContext<State>| async move {
                    Ok(page(&ctx))
                })
                .middleware(MemberOfTeamA),
            )
        })
        .and_then(|router| {
            // A page that checks access in its loader and answers a denial as a 200 page.
            router.route(Route::html(
                "/{team}/overzicht",
                |ctx: HttpContext<State>| async move {
                    if ctx.params().get("team") == Some("t-a") {
                        ctx.affirm("team");
                        return Ok(page(&ctx));
                    }
                    Ok("not yours".to_owned())
                },
            ))
        })
        .and_then(|router| {
            router.route(Route::html(
                "/aanmelden",
                |_ctx: HttpContext<State>| async move { Ok("sign up".to_owned()) },
            ))
        })
        .expect("a well-declared router")
}

async fn get(router: &Router<State>, path: &str) -> ServerResponse {
    router
        .dispatch(ServerRequest::new(Method::Get, path, State))
        .await
}

fn header<'a>(response: &'a ServerResponse, name: &str) -> Option<&'a str> {
    response
        .headers()
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

#[tokio::test]
async fn a_guard_sees_the_real_id() {
    let response = get(&router(), "/sounds-global/aanvragen/12").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.body(), b"t-a:b-12");
}

#[tokio::test]
async fn a_variant_redirects_once_after_authorization() {
    let router = router();
    for variant in [
        "/old-name/aanvragen/12",
        "/sounds-global/aanvragen/012",
        "/old-name/aanvragen/012",
    ] {
        let response = get(&router, variant).await;
        assert_eq!(response.status(), 301, "{variant}");
        assert_eq!(
            header(&response, "location"),
            Some("/sounds-global/aanvragen/12"),
            "{variant}"
        );
        assert_eq!(
            header(&response, "cache-control"),
            Some("private, max-age=86400")
        );
        let followed = get(&router, "/sounds-global/aanvragen/12").await;
        assert_eq!(followed.status(), 200, "one hop reaches the page");
    }
}

#[tokio::test]
async fn the_query_survives_the_redirect() {
    let response = router()
        .dispatch(
            ServerRequest::new(Method::Get, "/old-name/aanvragen/12", State)
                .with_query("tab=files"),
        )
        .await;
    assert_eq!(
        header(&response, "location"),
        Some("/sounds-global/aanvragen/12?tab=files")
    );
}

#[tokio::test]
async fn a_cross_scope_variant_answers_like_unknown() {
    let router = router();
    let unknown = get(&router, "/nobody/aanvragen/1").await;
    assert_eq!(
        unknown.status(),
        401,
        "a void id is denied like a foreign record"
    );
    for probe in [
        "/rival-old/aanvragen/1",
        "/rival/aanvragen/01",
        "/rival/aanvragen/1",
        "/rival-old",
    ] {
        let response = get(&router, probe).await;
        assert_eq!(response.status(), unknown.status(), "{probe}");
        assert_eq!(response.body(), unknown.body(), "{probe}");
        assert_eq!(
            header(&response, "location"),
            None,
            "{probe}: nothing disclosed"
        );
    }
    let unknown_page = get(&router, "/nobody/overzicht").await;
    assert_eq!(unknown_page.status(), 200);
    for probe in ["/rival-old/overzicht", "/rival/overzicht"] {
        let response = get(&router, probe).await;
        assert_eq!(response.status(), unknown_page.status(), "{probe}");
        assert_eq!(response.body(), unknown_page.body(), "{probe}");
        assert_eq!(
            header(&response, "location"),
            None,
            "{probe}: nothing disclosed"
        );
    }
    let own = get(&router, "/old-name/overzicht").await;
    assert_eq!(
        own.status(),
        301,
        "a loader's affirmation earns the redirect"
    );
    assert_eq!(header(&own, "location"), Some("/sounds-global/overzicht"));
}

#[tokio::test]
async fn a_slug_cannot_reach_another_scopes_record() {
    // Aanvraag 1 is the rival's; under the reader's own team it names nothing.
    let response = get(&router(), "/sounds-global/aanvragen/1").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.body(), b"t-a:void");
}

#[tokio::test]
async fn an_unaffirmed_variant_is_served_in_place() {
    let router = Router::new()
        .segment(TeamSlugs)
        .and_then(|router| {
            router.route(Route::html(
                "/{team}",
                |ctx: HttpContext<State>| async move { Ok(page(&ctx)) },
            ))
        })
        .expect("a well-declared router");
    let response = get(&router, "/old-name").await;
    assert_eq!(
        response.status(),
        200,
        "nothing affirmed the team, so nothing moves"
    );
    assert_eq!(header(&response, "location"), None);
}

#[tokio::test]
async fn a_post_resolves_in_place() {
    let router = Router::new()
        .segment(TeamSlugs)
        .and_then(|router| {
            router.route(
                Route::post(
                    "/{team}/naam",
                    |ctx: HttpContext<State>, _: Option<()>| async move {
                        Ok(ctx.params().get("team").unwrap_or_default().to_owned())
                    },
                )
                .middleware(MemberOfTeamA),
            )
        })
        .expect("a well-declared router");
    let response = router
        .dispatch(ServerRequest::new(Method::Post, "/old-name/naam", State))
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.body(), b"\"t-a\"");
}

#[tokio::test]
async fn a_store_failure_is_an_error_never_a_void() {
    let response = get(&router(), "/outage").await;
    assert_eq!(response.status(), 503);
}

#[tokio::test]
async fn a_literal_route_outranks_a_parameter_registered_before_it() {
    let response = get(&router(), "/aanmelden").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.body(), b"sign up");
}

#[test]
fn two_routes_of_one_shape_are_ambiguous() {
    let result = Router::<State>::new()
        .route(Route::html(
            "/{team}",
            |_ctx: HttpContext<State>| async move { Ok(String::new()) },
        ))
        .and_then(|router| {
            router.route(Route::html(
                "/{slug}",
                |_ctx: HttpContext<State>| async move { Ok(String::new()) },
            ))
        });
    let error = result.err().expect("an ambiguous route is refused");
    assert!(matches!(error, ServerError::RouteAmbiguous { .. }));
    assert_eq!(fundamental_core::Error::error_code(&error), "API_029");
}

#[tokio::test]
async fn a_retired_template_redirects_once_with_what_was_asked() {
    let router = router()
        .retire("/teams/{team}", "/{team}")
        .expect("the new template captures nothing the old one lacks");
    let response = get(&router, "/teams/old-name").await;
    assert_eq!(response.status(), 301);
    assert_eq!(header(&response, "location"), Some("/old-name"));
}

#[test]
fn a_retired_template_must_carry_every_capture() {
    let result = router().retire("/teams", "/{team}");
    assert!(matches!(
        result.err(),
        Some(ServerError::SegmentsMisdeclared { .. })
    ));
}

#[test]
fn a_scope_must_be_declared_and_come_first() {
    let router = Router::<State>::new()
        .segment(BuildNumbers)
        .and_then(|router| {
            router.route(Route::html(
                "/aanvragen/{build}",
                |_ctx: HttpContext<State>| async move { Ok(String::new()) },
            ))
        })
        .expect("registration itself succeeds");
    let problems = router.segment_problems();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        router
            .segment_problems()
            .iter()
            .any(|p| p.contains("not declared"))
    );
    assert!(
        router
            .segment_problems()
            .iter()
            .any(|p| p.contains("without `team` before it"))
    );
}

#[test]
fn a_parameter_is_declared_once() {
    let result = Router::<State>::new()
        .segment(TeamSlugs)
        .and_then(|router| router.segment(TeamSlugs));
    assert!(matches!(
        result.err(),
        Some(ServerError::SegmentsMisdeclared { .. })
    ));
}

#[tokio::test]
async fn a_return_path_carries_what_was_asked_not_the_id() {
    let router = Router::new()
        .segment(TeamSlugs)
        .and_then(|router| {
            router.route(Route::html(
                "/{team}/stap",
                |ctx: HttpContext<State>| async move {
                    Ok(fundamental_server::request_return_path(&ctx)
                        .map(|path| path.as_location().to_owned())
                        .unwrap_or_default())
                },
            ))
        })
        .expect("a well-declared router");
    let response = get(&router, "/old-name/stap").await;
    assert_eq!(response.body(), b"/old-name/stap");
}

#[test]
fn the_literal_words_are_every_word_the_router_answers_at() {
    let words = router().literal_segments();
    assert!(words.contains("aanvragen"));
    assert!(words.contains("aanmelden"));
    assert!(!words.iter().any(|word| word.starts_with('{')), "{words:?}");
}

#[tokio::test]
async fn an_undeclared_capture_keeps_its_spelling_in_the_redirect() {
    let router = Router::new()
        .segment(TeamSlugs)
        .and_then(|router| {
            router.route(
                Route::html("/{team}/{tab}", |ctx: HttpContext<State>| async move {
                    Ok(page(&ctx))
                })
                .middleware(MemberOfTeamA),
            )
        })
        .expect("a well-declared router");
    let response = get(&router, "/old-name/files").await;
    assert_eq!(response.status(), 301);
    assert_eq!(header(&response, "location"), Some("/sounds-global/files"));
}

/// A guard whose route is personal: it allows team `t-a` and says nothing may be kept.
struct PersonalToTeamA;

impl Middleware<State> for PersonalToTeamA {
    async fn handle(
        &self,
        ctx: HttpContext<State>,
        body: Vec<u8>,
        next: Next<'_, State>,
    ) -> MiddlewareResult {
        let headers = ctx.response_headers();
        let answer = MemberOfTeamA.handle(ctx, body, next).await;
        headers.replace("cache-control", "no-store");
        answer
    }
}

#[tokio::test]
async fn a_redirect_from_a_personal_route_carries_one_cache_control() {
    let router = Router::new()
        .segment(TeamSlugs)
        .and_then(|router| router.segment(BuildNumbers))
        .and_then(|router| {
            router.route(
                Route::html(
                    "/{team}/aanvragen/{build}",
                    |ctx: HttpContext<State>| async move {
                        ctx.affirm("build");
                        Ok(page(&ctx))
                    },
                )
                .middleware(PersonalToTeamA),
            )
        })
        .expect("the router builds");
    let response = get(&router, "/old-name/aanvragen/12").await;
    assert_eq!(response.status(), 301);
    let cache_control: Vec<&str> = response
        .headers()
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("cache-control"))
        .map(|(_, value)| value.as_str())
        .collect();
    assert_eq!(cache_control, ["no-store"]);
}
