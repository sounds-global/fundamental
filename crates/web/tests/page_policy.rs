//! Declared page policies over real HTTP: every guard runs before its loader, and a personal page is never cached.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use fundamental_auth::{
    AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier, UserId,
};
use fundamental_authz::AuthzError;
use fundamental_authz::authorizer::{
    CheckResponse, DynAuthorizer, ExpandResponse, HasAuthorizer, ListObjectsResponse,
};
use fundamental_authz::middleware::Authorized;
use fundamental_authz::schema::{NamespaceName, RelationName, SchemaHash};
use fundamental_authz::tuple::{CaveatContext, ObjectRef, RelationTuple, SubjectRef};
use fundamental_authz::zookie::{BackendTag, Consistency, HybridTs, Zookie};
use fundamental_core::AnyError;
use fundamental_core::secrets::SecretString;
use fundamental_macros::{Error, Namespace, PublicSegment, Validate};
use fundamental_server::{LoginRoute, PageAuth, PublicRoute, ServerListener};
use fundamental_web::prelude::*;
use fundamental_web::{
    Cache, CredentialSource, GateReason, HttpContext, LoadCx, PageContext, PagePolicy, PageRefusal,
    PageRoute, PolicyChain, PublicSegment as _, Refused, Route, Router, ServerConfig, WebServer,
    at, render_page,
};

#[derive(Namespace)]
enum User {}

#[derive(Namespace)]
enum Team {
    #[relation(direct(User))]
    Member,
    #[relation(direct(User))]
    Admin,
}

#[derive(Namespace)]
enum Project {
    #[relation(direct(User))]
    Viewer,
}

#[derive(Default)]
struct StubAuthorizer {
    grants: Mutex<BTreeSet<String>>,
    failing: AtomicBool,
    log: Arc<Mutex<Vec<String>>>,
}

impl StubAuthorizer {
    fn grant(&self, object: &str, relation: &str, user: &str) {
        lock(&self.grants).insert(format!("{object}#{relation}@user:{user}"));
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn zookie() -> Zookie {
    Zookie::new(BackendTag::Offline, HybridTs::ZERO, SchemaHash::empty())
}

type Boxed<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, AuthzError>> + Send + 'a>>;

impl DynAuthorizer for StubAuthorizer {
    fn check<'a>(
        &'a self,
        object: ObjectRef,
        relation: RelationName,
        subject: SubjectRef,
        _context: CaveatContext,
        _consistency: Consistency,
    ) -> Boxed<'a, CheckResponse> {
        let question = format!("{object}#{}@{subject}", relation.as_str());
        lock(&self.log).push(format!("check {question}"));
        let answer = if self.failing.load(Ordering::SeqCst) {
            Err(AuthzError::StorageDecode("the tuple store is down".into()))
        } else {
            Ok(CheckResponse {
                allowed: lock(&self.grants).contains(&question),
                evaluated_at: zookie(),
            })
        };
        Box::pin(async move { answer })
    }

    fn expand<'a>(
        &'a self,
        _object: ObjectRef,
        _relation: RelationName,
        _consistency: Consistency,
    ) -> Boxed<'a, ExpandResponse> {
        Box::pin(async { Err(AuthzError::StorageDecode("not in this stub".into())) })
    }

    fn list_objects<'a>(
        &'a self,
        _subject: SubjectRef,
        _object_namespace: NamespaceName,
        _relation: RelationName,
        _context: CaveatContext,
        _consistency: Consistency,
        _limit: usize,
    ) -> Boxed<'a, ListObjectsResponse> {
        Box::pin(async { Err(AuthzError::StorageDecode("not in this stub".into())) })
    }

    fn write<'a>(&'a self, _tuples: &'a [RelationTuple]) -> Boxed<'a, Zookie> {
        Box::pin(async { Err(AuthzError::StorageDecode("not in this stub".into())) })
    }

    fn delete<'a>(&'a self, _tuples: &'a [RelationTuple]) -> Boxed<'a, Zookie> {
        Box::pin(async { Err(AuthzError::StorageDecode("not in this stub".into())) })
    }
}

#[derive(Clone)]
struct AppState {
    authz: Arc<StubAuthorizer>,
    loads: Arc<AtomicUsize>,
    log: Arc<Mutex<Vec<String>>>,
}

impl HasAuthorizer for AppState {
    fn authorizer(&self) -> &dyn DynAuthorizer {
        self.authz.as_ref()
    }
}

impl AppState {
    fn new() -> Self {
        let log = Arc::new(Mutex::new(Vec::new()));
        let authz = StubAuthorizer {
            log: Arc::clone(&log),
            ..StubAuthorizer::default()
        };
        authz.grant("team:t1", "member", "alice");
        authz.grant("team:t1", "admin", "carol");
        authz.grant("team:t1", "member", "carol");
        Self {
            authz: Arc::new(authz),
            loads: Arc::new(AtomicUsize::new(0)),
            log,
        }
    }

    fn loads(&self) -> usize {
        self.loads.load(Ordering::SeqCst)
    }

    fn log(&self) -> Vec<String> {
        lock(&self.log).clone()
    }
}

struct Overview {
    team: String,
}

async fn load_team(
    cx: LoadCx<AppState>,
    team: Authorized<Team, team_relation::Member>,
) -> Result<Overview, PageRefusal> {
    cx.state().loads.fetch_add(1, Ordering::SeqCst);
    lock(&cx.state().log).push("load".to_owned());
    Ok(Overview {
        team: team.id().to_owned(),
    })
}

async fn load_team_as_admin(
    _cx: LoadCx<AppState>,
    team: Authorized<Team, team_relation::Admin>,
) -> Result<Overview, PageRefusal> {
    Ok(Overview {
        team: team.id().to_owned(),
    })
}

async fn load_nothing_there(cx: LoadCx<AppState>) -> Result<Overview, PageRefusal> {
    cx.state().loads.fetch_add(1, Ordering::SeqCst);
    Err(PageRefusal::Unavailable)
}

#[derive(Debug, Error)]
enum DomainError {
    #[error(code = "DOM_403", message = "Not yours", status = 403)]
    NotYours,
}

async fn load_someone_elses(
    cx: LoadCx<AppState>,
    _team: Authorized<Team, team_relation::Member>,
) -> Result<Overview, PageRefusal> {
    cx.state().loads.fetch_add(1, Ordering::SeqCst);
    Err(DomainError::NotYours)?
}

#[derive(Clone, Copy, PublicSegment)]
enum Tab {
    Overview,
    #[segment("bestanden")]
    Files,
}

async fn load_tab(
    cx: LoadCx<AppState>,
    team: Authorized<Team, team_relation::Member>,
) -> Result<Overview, PageRefusal> {
    let tab: Tab = cx.public_segment("tab")?;
    Ok(Overview {
        team: format!("{} {}", team.id(), tab.segment()),
    })
}

struct TeamFrame {
    label: String,
}

async fn frame_team(
    _cx: LoadCx<AppState>,
    team: Authorized<Team, team_relation::Member>,
) -> Result<TeamFrame, PageRefusal> {
    Ok(TeamFrame {
        label: format!("Frame of {}", team.id()),
    })
}

async fn frame_nothing_there(
    _cx: LoadCx<AppState>,
    _team: Authorized<Team, team_relation::Member>,
) -> Result<TeamFrame, PageRefusal> {
    Err(PageRefusal::Unavailable)
}

fn framed_layout(frame: TeamFrame, children: impl IntoView + 'static) -> impl IntoView {
    view! { <aside>{frame.label}</aside> <main>{children}</main> }
}

async fn load_after_step_up(
    _cx: LoadCx<AppState>,
    _team: Authorized<Team, team_relation::Member>,
) -> Result<Overview, PageRefusal> {
    Err(PageRefusal::StepUp)
}

#[derive(serde::Deserialize, Validate)]
struct Search {
    #[validate(length(min = 2))]
    q: String,
}

async fn load_search(cx: LoadCx<AppState>) -> Result<Overview, PageRefusal> {
    let search: Search = cx.query()?;
    Ok(Overview { team: search.q })
}

async fn load_slug(cx: LoadCx<AppState>) -> Result<Overview, PageRefusal> {
    Ok(Overview {
        team: cx.public_param("slug")?.to_owned(),
    })
}

async fn load_undeclared(cx: LoadCx<AppState>) -> Result<Overview, PageRefusal> {
    Ok(Overview {
        team: cx.public_param("other")?.to_owned(),
    })
}

fn team_page(model: Overview) -> impl IntoView {
    let state = use_context::<AppState>().is_some();
    let page = use_context::<PageContext>().is_some();
    view! {
        <h1>{format!("Team {}", model.team)}</h1>
        <p>{format!("state={state} page={page}")}</p>
    }
}

fn home_page() -> impl IntoView {
    view! { <h1>"Home"</h1> }
}

fn gate_page(refused: Refused) -> impl IntoView {
    let reason = match refused.reason() {
        GateReason::SignIn => "sign-in",
        GateReason::StepUp => "step-up",
        _ => "forbidden",
    };
    view! { <h1>{format!("Gate {} {reason}", refused.status())}</h1> }
}

fn document(ctx: &PageContext, body: &str) -> String {
    let title = ctx.head().title.clone().unwrap_or_default();
    format!("<!DOCTYPE html><html><head><title>{title}</title></head>{body}</html>")
}

fn root() -> PolicyChain {
    PolicyChain::root().then(
        "policy.rs",
        PagePolicy::new()
            .cache(Cache::Public {
                max_age: Duration::from_secs(600),
            })
            .title("Acme"),
    )
}

fn team_chain() -> PolicyChain<fundamental_web::Guarded> {
    root().then(
        "teams/[team]/policy.rs",
        PagePolicy::new()
            .signed_in()
            .authorize(Team::Member)
            .object("team")
            .title("Team"),
    )
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn router() -> Result<Router<AppState>, AnyError> {
    let router = Router::new()
        .route(PageRoute::new("/home", root())?.view(home_page, document))?
        .route(PageRoute::new("/teams/{team}", team_chain())?.loaded(
            (at("teams/[team]/loader.rs", load_team),),
            |(model,)| team_page(model),
            document,
        )?)?
        // A deeper `policy.rs` that declares nothing still runs the guard above it.
        .route(
            PageRoute::new(
                "/teams/{team}/settings",
                team_chain().then("teams/[team]/settings/policy.rs", PagePolicy::new()),
            )?
            .loaded(
                (at("teams/[team]/settings/loader.rs", load_team),),
                |(model,)| team_page(model),
                document,
            )?,
        )?
        .route(
            PageRoute::new(
                "/teams/{team}/{tab}",
                team_chain().then(
                    "teams/[team]/[tab]/policy.rs",
                    PagePolicy::new().public_segment::<Tab>("tab"),
                ),
            )?
            .loaded(
                (at("teams/[team]/[tab]/loader.rs", load_tab),),
                |(model,)| team_page(model),
                document,
            )?,
        )?
        // On `PageAuth`'s public list, so an anonymous reader reaches the guard itself.
        .route(
            PageRoute::new(
                "/open/{team}",
                root().then(
                    "open/[team]/policy.rs",
                    PagePolicy::new().authorize(Team::Member).object("team"),
                ),
            )?
            .loaded(
                (at("open/[team]/loader.rs", load_team),),
                |(model,)| team_page(model),
                document,
            )?,
        )?
        .route(
            PageRoute::new(
                "/missing/{team}",
                root().then(
                    "missing/[team]/policy.rs",
                    PagePolicy::new().authorize(Team::Member).object("team"),
                ),
            )?
            .loaded(
                (at("missing/[team]/loader.rs", load_nothing_there),),
                |(model,)| team_page(model),
                document,
            )?,
        )?
        .route(
            PageRoute::new(
                "/theirs/{team}",
                root().then(
                    "theirs/[team]/policy.rs",
                    PagePolicy::new().authorize(Team::Member).object("team"),
                ),
            )?
            .loaded(
                (at("theirs/[team]/loader.rs", load_someone_elses),),
                |(model,)| team_page(model),
                document,
            )?,
        )?
        .route(
            PageRoute::new(
                "/framed/{team}",
                root().then(
                    "framed/[team]/policy.rs",
                    PagePolicy::new().authorize(Team::Member).object("team"),
                ),
            )?
            .loaded(
                (
                    at("framed/[team]/loader.rs", load_team),
                    at("framed/[team]/loader.rs", frame_team),
                ),
                |(model, frame)| framed_layout(frame, team_page(model)),
                document,
            )?,
        )?
        .route(
            PageRoute::new(
                "/unframed/{team}",
                root().then(
                    "unframed/[team]/policy.rs",
                    PagePolicy::new().authorize(Team::Member).object("team"),
                ),
            )?
            .loaded(
                (
                    at("unframed/[team]/loader.rs", load_team),
                    at("unframed/[team]/loader.rs", frame_nothing_there),
                ),
                |(model, frame)| framed_layout(frame, team_page(model)),
                document,
            )?,
        )?
        .route(
            PageRoute::new(
                "/confirm/{team}",
                root().then(
                    "confirm/[team]/policy.rs",
                    PagePolicy::new().authorize(Team::Member).object("team"),
                ),
            )?
            .loaded(
                (at("confirm/[team]/loader.rs", load_after_step_up),),
                |(model,)| team_page(model),
                document,
            )?,
        )?
        .route(PageRoute::new("/search", root())?.loaded(
            (at("search/loader.rs", load_search),),
            |(model,)| team_page(model),
            document,
        )?)?
        .route(
            PageRoute::new(
                "/blog/{slug}",
                root().then(
                    "blog/[slug]/policy.rs",
                    PagePolicy::new().public_param("slug"),
                ),
            )?
            .loaded(
                (at("blog/[slug]/loader.rs", load_slug),),
                |(model,)| team_page(model),
                document,
            )?,
        )?
        .route(
            PageRoute::new(
                "/drafts/{slug}",
                root().then(
                    "drafts/[slug]/policy.rs",
                    PagePolicy::new().public_param("slug"),
                ),
            )?
            .loaded(
                (at("drafts/[slug]/loader.rs", load_undeclared),),
                |(model,)| team_page(model),
                document,
            )?,
        )?
        .not_found(Route::html(
            "__not_found",
            |ctx: HttpContext<AppState>| async move {
                let rendered = render_page(&ctx, || view! { <h1>"Not found"</h1> });
                Ok(document(rendered.context(), rendered.body()))
            },
        ))
        .error_page(Route::html(
            "__error",
            |ctx: HttpContext<AppState>| async move {
                let status = ctx.shown_error().map_or(0, |shown| shown.status());
                let rendered =
                    render_page(&ctx, move || view! { <h1>{format!("Error {status}")}</h1> });
                Ok(document(rendered.context(), rendered.body()))
            },
        ))
        .gate_page(PageRoute::gate(gate_page, document))
        .middleware(PageAuth::new(
            LoginRoute::new("/login"),
            &PublicRoute::exactly([
                "/home",
                "/open/{team}",
                "/missing/{team}",
                "/theirs/{team}",
                "/search",
                "/blog/{slug}",
                "/drafts/{slug}",
            ]),
        ));
    Ok(router)
}

const SESSION: &str = "__Host-session";

struct Sessions;

impl PrincipalVerifier for Sessions {
    async fn verify(&self, token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        match token.expose().as_str() {
            user @ ("alice" | "bob" | "carol") => Ok(AuthenticatedPrincipal {
                user_id: UserId::new(user),
                device_id: None,
                scopes: Vec::new(),
                expires_at: 0,
            }),
            _ => Err(AuthError::Invalid(InvalidToken::Unrecognised)),
        }
    }
}

struct Served {
    listener: fundamental_web::WebListener,
    state: AppState,
}

async fn serve() -> Served {
    let state = AppState::new();
    let config = ServerConfig::new("127.0.0.1:0".parse().expect("an address"))
        .with_credential_source(CredentialSource::cookie(SESSION));
    let listener = WebServer::new(config, state.clone(), router().expect("the routes compose"))
        .with_verifier(Sessions)
        .serve()
        .await
        .expect("binds an ephemeral port");
    Served { listener, state }
}

struct Answer {
    status: u16,
    cache_control: Vec<String>,
    retry_after: Option<String>,
    location: Option<String>,
    body: String,
}

impl Served {
    async fn navigate(&self, path: &str, user: Option<&str>) -> Answer {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("a client");
        let mut request = client
            .get(format!("http://{}{path}", self.listener.local_addr()))
            .header("sec-fetch-mode", "navigate")
            .header("accept", "text/html");
        if let Some(user) = user {
            request = request.header("cookie", format!("{SESSION}={user}"));
        }
        let response = request.send().await.expect("a response");
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        Answer {
            status: response.status().as_u16(),
            cache_control: response
                .headers()
                .get_all("cache-control")
                .iter()
                .filter_map(|value| value.to_str().ok().map(str::to_owned))
                .collect(),
            retry_after: header("retry-after"),
            location: header("location"),
            body: response.text().await.expect("a body"),
        }
    }

    async fn stop(self) {
        self.listener.stop().await.expect("stops cleanly");
    }
}

#[tokio::test]
async fn guards_run_before_the_loader_and_cannot_be_removed() {
    let served = serve().await;

    for path in ["/teams/t1", "/teams/t1/settings"] {
        let signed_out = served.navigate(path, None).await;
        assert_eq!(signed_out.status, 303, "{path}: sent to sign in");
        assert!(
            signed_out
                .location
                .as_deref()
                .is_some_and(|l| l.starts_with("/login")),
            "{path}: {:?}",
            signed_out.location
        );

        let outsider = served.navigate(path, Some("bob")).await;
        assert_eq!(outsider.status, 403, "{path}: {}", outsider.body);
        assert!(
            outsider.body.contains("Gate 403 forbidden"),
            "{}",
            outsider.body
        );
    }
    assert_eq!(
        served.state.loads(),
        0,
        "no loader ran for a refused request"
    );
    assert!(
        served
            .state
            .log()
            .iter()
            .all(|entry| entry.starts_with("check ")),
        "{:?}",
        served.state.log()
    );

    // The settings page declares nothing of its own, and still the team guard ran.
    let member = served.navigate("/teams/t1/settings", Some("alice")).await;
    assert_eq!(member.status, 200, "{}", member.body);
    assert!(member.body.contains("Team t1"), "{}", member.body);
    assert_eq!(served.state.loads(), 1);
    let log = served.state.log();
    let checked = log
        .iter()
        .position(|entry| entry == "check team:t1#member@user:alice")
        .expect("the member check ran");
    let loaded = log
        .iter()
        .position(|entry| entry == "load")
        .expect("the loader ran");
    assert!(checked < loaded, "the guard ran before the loader: {log:?}");

    served.stop().await;
}

#[tokio::test]
async fn a_signed_out_request_gets_the_gate_and_no_load() {
    let served = serve().await;
    let answer = served.navigate("/open/t1", None).await;
    assert_eq!(answer.status, 401, "{}", answer.body);
    assert!(answer.body.contains("Gate 401 sign-in"), "{}", answer.body);
    assert!(
        answer.body.contains("<title></title>"),
        "the gate shows nothing the refused route declared: {}",
        answer.body
    );
    assert_eq!(answer.cache_control, ["no-store"]);
    assert_eq!(served.state.loads(), 0);
    served.stop().await;
}

#[tokio::test]
async fn a_missing_relation_gets_the_403_gate_and_no_load() {
    let served = serve().await;
    let answer = served.navigate("/teams/t1", Some("bob")).await;
    assert_eq!(answer.status, 403);
    assert!(
        answer.body.contains("Gate 403 forbidden"),
        "{}",
        answer.body
    );
    assert!(
        !answer.body.contains("Team t1"),
        "nothing of the team is shown"
    );
    assert_eq!(answer.cache_control, ["no-store"]);
    assert_eq!(served.state.loads(), 0);
    served.stop().await;
}

#[tokio::test]
async fn a_loader_page_has_no_state_in_context() {
    let served = serve().await;
    let answer = served.navigate("/teams/t1", Some("alice")).await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert!(
        answer.body.contains("state=false page=true"),
        "{}",
        answer.body
    );
    served.stop().await;
}

#[tokio::test]
async fn policy_cache_and_title_reach_every_page_below() {
    let served = serve().await;
    let home = served.navigate("/home", None).await;
    assert_eq!(home.status, 200);
    assert_eq!(home.cache_control, ["public, max-age=600"]);
    assert!(home.body.contains("<title>Acme</title>"), "{}", home.body);

    let team = served.navigate("/teams/t1", Some("alice")).await;
    assert_eq!(team.status, 200);
    assert!(
        team.body.contains("<title>Team</title>"),
        "the innermost title: {}",
        team.body
    );
    served.stop().await;
}

#[tokio::test]
async fn a_personal_page_cannot_be_public() {
    let served = serve().await;
    let guarded = served.navigate("/teams/t1", Some("alice")).await;
    assert_eq!(guarded.status, 200);
    assert_eq!(
        guarded.cache_control,
        ["no-store"],
        "a guarded route under a public root is never stored"
    );

    let signed_in_reader = served.navigate("/home", Some("alice")).await;
    assert_eq!(signed_in_reader.status, 200);
    assert_eq!(
        signed_in_reader.cache_control,
        ["no-store"],
        "a public page read by someone signed in is personal"
    );
    served.stop().await;
}

#[tokio::test]
async fn every_loader_refusal_is_one_not_found() {
    let served = serve().await;
    let absent = served.navigate("/missing/t1", Some("alice")).await;
    let not_yours = served.navigate("/theirs/t1", Some("alice")).await;
    let unknown_tab = served.navigate("/teams/t1/settingz", Some("alice")).await;
    for answer in [&absent, &not_yours, &unknown_tab] {
        assert_eq!(answer.status, 404, "{}", answer.body);
        assert!(answer.body.contains("Not found"), "{}", answer.body);
        assert_eq!(answer.cache_control, ["no-store"]);
    }
    assert_eq!(
        absent.body, not_yours.body,
        "nothing tells absent from someone else's"
    );

    let tab = served.navigate("/teams/t1/bestanden", Some("alice")).await;
    assert_eq!(tab.status, 200);
    assert!(tab.body.contains("Team t1 bestanden"), "{}", tab.body);
    served.stop().await;
}

#[tokio::test]
async fn a_failing_authorizer_answers_503() {
    let served = serve().await;
    served.state.authz.failing.store(true, Ordering::SeqCst);
    let answer = served.navigate("/teams/t1", Some("alice")).await;
    assert_eq!(answer.status, 503, "{}", answer.body);
    assert_eq!(answer.retry_after.as_deref(), Some("5"));
    assert!(
        answer.body.contains("Error 503"),
        "the error page, never the gate: {}",
        answer.body
    );
    assert_eq!(answer.cache_control, ["no-store"]);
    assert_eq!(served.state.loads(), 0);
    served.stop().await;
}

fn boot_code(result: Result<Route<AppState>, AnyError>) -> &'static str {
    match result {
        Ok(_) => "booted",
        Err(error) => error.get().error_code(),
    }
}

#[test]
fn a_loader_stronger_than_its_guard_refuses_to_boot() {
    let route = PageRoute::new("/teams/{team}", team_chain()).and_then(|route| {
        route.loaded(
            (at("teams/[team]/loader.rs", load_team_as_admin),),
            |(model,)| team_page(model),
            document,
        )
    });
    assert_eq!(boot_code(route), "FW_PAGE_001");
}

#[test]
fn two_objects_in_one_namespace_refuse_to_boot() {
    let chain = team_chain().then(
        "teams/[team]/[other]/policy.rs",
        PagePolicy::new().authorize(Team::Member).object("other"),
    );
    let route =
        PageRoute::new("/teams/{team}/{other}", chain).map(|route| route.view(home_page, document));
    assert_eq!(boot_code(route), "FW_PAGE_002");
}

#[test]
fn an_unguarded_param_refuses_to_boot() {
    let route = PageRoute::new("/teams/{team}/{project}", team_chain())
        .map(|route| route.view(home_page, document));
    assert_eq!(boot_code(route), "FW_PAGE_003");
}

#[test]
fn a_guard_on_an_absent_param_refuses_to_boot() {
    let route = PageRoute::new("/teams", team_chain()).map(|route| route.view(home_page, document));
    assert_eq!(boot_code(route), "FW_PAGE_004");
}

#[test]
fn a_raw_public_param_under_an_object_guard_refuses_to_boot() {
    let chain = team_chain().then(
        "teams/[team]/[project]/policy.rs",
        PagePolicy::new().public_param("project"),
    );
    let route = PageRoute::new("/teams/{team}/{project}", chain)
        .map(|route| route.view(home_page, document));
    assert_eq!(boot_code(route), "FW_PAGE_005");

    // A raw parameter under a role over every record ranges over every id by definition.
    let guarded = team_chain().then(
        "teams/[team]/[project]/policy.rs",
        PagePolicy::new()
            .authorize(Project::Viewer)
            .object("project"),
    );
    assert!(PageRoute::new("/teams/{team}/{project}", guarded).is_ok());
    let staff = root().then(
        "staff/policy.rs",
        PagePolicy::new()
            .authorize(Team::Admin)
            .object_const("platform")
            .public_param("account"),
    );
    assert!(PageRoute::new("/staff/{account}", staff).is_ok());
}

#[tokio::test]
async fn a_layout_frame_reaches_its_layout() {
    let served = serve().await;
    let framed = served.navigate("/framed/t1", Some("alice")).await;
    assert_eq!(framed.status, 200, "{}", framed.body);
    assert!(
        framed.body.contains("<aside>Frame of t1</aside>"),
        "{}",
        framed.body
    );
    assert!(framed.body.contains("Team t1"), "{}", framed.body);

    let refused = served.navigate("/unframed/t1", Some("alice")).await;
    assert_eq!(
        refused.status, 404,
        "a frame's refusal is the one not found"
    );
    assert!(refused.body.contains("Not found"), "{}", refused.body);
    served.stop().await;
}

#[tokio::test]
async fn a_loader_asking_to_step_up_gets_the_gate() {
    let served = serve().await;
    let answer = served.navigate("/confirm/t1", Some("alice")).await;
    assert_eq!(answer.status, 401, "{}", answer.body);
    assert!(answer.body.contains("Gate 401 step-up"), "{}", answer.body);
    assert_eq!(answer.cache_control, ["no-store"]);
    served.stop().await;
}

#[tokio::test]
async fn a_query_that_does_not_fit_is_a_400() {
    let served = serve().await;
    let fits = served.navigate("/search?q=sounds", None).await;
    assert_eq!(fits.status, 200, "{}", fits.body);
    assert!(fits.body.contains("Team sounds"), "{}", fits.body);

    let too_short = served.navigate("/search?q=s", None).await;
    assert_eq!(too_short.status, 400, "{}", too_short.body);
    assert!(too_short.body.contains("Error 400"), "{}", too_short.body);

    let missing = served.navigate("/search", None).await;
    assert_eq!(missing.status, 400, "{}", missing.body);
    served.stop().await;
}

#[tokio::test]
async fn a_loader_reads_only_the_parameters_declared_public() {
    let served = serve().await;
    let declared = served.navigate("/blog/hello", None).await;
    assert_eq!(declared.status, 200, "{}", declared.body);
    assert!(declared.body.contains("Team hello"), "{}", declared.body);

    let undeclared = served.navigate("/drafts/hello", None).await;
    assert_eq!(undeclared.status, 500, "{}", undeclared.body);
    assert!(undeclared.body.contains("Error 500"), "{}", undeclared.body);
    served.stop().await;
}
