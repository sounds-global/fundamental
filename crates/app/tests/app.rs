//! An app, as its tests answer it and as a deploy checks it: one value, no listener started.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fundamental_app::{App, AppPart, Served, SyncStore};
use fundamental_config::AppSettings;
use fundamental_core::address::WebOrigin;
use fundamental_core::i18n::Locale;
use fundamental_core::{AnyError, Environment, ServiceDeclaration};
use fundamental_nats::{Message, NatsResult, ReceivedMessage};
use fundamental_server::{
    CspSource, HttpContext, Indexing, LocalePolicy, LocaleSource, Method, PreferencePolicy,
    RequestHeaders, Route, Router, ServerConfig, ServerError, ServerRequest, ServerResponse,
};
use fundamental_web::{Document, ShareCard, SiteIcon, default_document, render_page};
use fundamental_ws::{WsConfig, WsRouter};
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

const ICONS: [SiteIcon; 1] = [SiteIcon::new("/icon.svg", "image/svg+xml", b"<svg/>").typed()];
const APPEARANCE: &[&str] = &["light", "dark"];

static LOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

fn install_look() -> Result<(), AnyError> {
    LOOK_INSTALLED.store(true, Ordering::SeqCst);
    Ok(())
}

fn register_pages(router: Router<()>) -> Result<Router<()>, ServerError> {
    let page = |path: &'static str| {
        Route::html(path, |ctx: HttpContext<()>| async move {
            let page = render_page(&ctx, || view! { <main>"page"</main> });
            page.context().set_title("A page");
            Ok(page.into_document(default_document))
        })
    };
    router
        .route(page("/"))?
        .route(page("/mission"))?
        .route(page("/review/article"))
}

fn languages() -> LocalePolicy {
    let english = Locale::new("en", None::<&str>);
    let dutch = Locale::new("nl", None::<&str>);
    LocalePolicy::new(english.clone())
        .with_supported(vec![english, dutch])
        .with_source(LocaleSource::cookie_then_accept_language("lang"))
        .with_region(Locale::new("en", Some("GB")))
        .with_region(Locale::new("nl", Some("NL")))
}

fn app() -> App<()> {
    App::new("www")
        .languages(languages())
        .appearance(PreferencePolicy::new("appearance", "theme", APPEARANCE))
        .pages(register_pages)
        .look(install_look)
        .icons(&ICONS)
        .document(Document::new().share(ShareCard::new("Example")))
        .unlisted("/review")
        .state(|_| Ok(()))
}

fn navigation(path: &str) -> ServerRequest<()> {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let headers: RequestHeaders = [
        ("sec-fetch-mode", "navigate"),
        ("sec-fetch-site", "none"),
        ("accept", "text/html"),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value.to_owned()))
    .collect();
    ServerRequest::new(Method::Get, path, ())
        .with_query(query)
        .with_headers(headers)
}

async fn answer(request: ServerRequest<()>) -> ServerResponse {
    app()
        .router()
        .unwrap_or_else(|error| panic!("{error:?}"))
        .dispatch(request)
        .await
        .buffered()
        .await
}

fn body(response: &ServerResponse) -> String {
    String::from_utf8_lossy(response.body()).into_owned()
}

fn header<'a>(response: &'a ServerResponse, name: &str) -> Option<&'a str> {
    response
        .headers()
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

#[tokio::test]
async fn the_router_answers_a_page_in_the_apps_shell_with_its_look_installed() {
    let response = answer(
        navigation("/mission")
            .with_locale(Some(Locale::new("nl", None::<&str>)))
            .with_preferences(vec![("appearance", "dark")]),
    )
    .await;
    let html = body(&response);
    assert!(
        LOOK_INSTALLED.load(Ordering::SeqCst),
        "the look is installed"
    );
    assert!(
        html.starts_with("<!DOCTYPE html><html lang=\"nl\" data-theme=\"dark\">"),
        "{html}"
    );
    assert!(
        html.contains(r#"<link rel="icon" href="/icon.svg""#),
        "{html}"
    );
    assert!(html.contains("content=\"nl_NL\""), "{html}");
    assert!(
        html.contains("data-wasm=\"/_fundamental/www_bg.wasm"),
        "{html}"
    );
}

#[tokio::test]
async fn the_router_serves_the_apps_icons() {
    let response = answer(navigation("/icon.svg")).await;
    assert_eq!(response.body(), b"<svg/>");
}

#[tokio::test]
async fn a_readers_choice_of_language_is_remembered() {
    let response = answer(navigation("/mission?lang=nl")).await;
    assert_eq!(response.status(), 303, "{}", body(&response));
    assert!(
        header(&response, "set-cookie").is_some_and(|cookie| cookie.starts_with("lang=nl")),
        "{:?}",
        response.headers()
    );
}

#[tokio::test]
async fn an_unlisted_page_asks_not_to_be_indexed() {
    let unlisted = answer(navigation("/review/article")).await;
    let listed = answer(navigation("/mission")).await;
    assert_eq!(header(&unlisted, "x-robots-tag"), Some("noindex"));
    assert_eq!(header(&listed, "x-robots-tag"), None);
}

#[test]
fn a_language_policy_that_remembers_nothing_is_refused() {
    let forgetful = LocalePolicy::new(Locale::new("en", None::<&str>));
    let Err(error) = App::<()>::new("www").languages(forgetful).router() else {
        panic!("a choice that cannot be remembered is refused");
    };
    assert_eq!(error.get().error_code(), "SERVE_001");
}

#[test]
fn the_app_lists_its_own_pages_but_the_unlisted() {
    let listed = app().listed_pages().expect("the router builds");
    assert_eq!(listed, ["/", "/mission"]);
}

#[test]
fn a_private_app_lists_no_page_and_asks_for_none_to_be_indexed() {
    let private = app().indexing(Indexing::Private);
    assert!(
        private
            .listed_pages()
            .expect("the router builds")
            .is_empty()
    );
    let config = private
        .server_config(listen())
        .expect("the hydration hash is well formed");
    let headers = config.security_headers().expect("the app's headers");
    assert_eq!(headers.indexing(), Indexing::Private);
}

#[test]
fn the_apps_headers_admit_the_hydration_script_and_its_font_sources() {
    let config = app()
        .font_source(CspSource::Origin(
            WebOrigin::parse("https://fonts.example").expect("an origin"),
        ))
        .server_config(listen())
        .expect("the hydration hash is well formed");
    let csp = config
        .security_headers()
        .expect("the app's headers")
        .headers()
        .into_iter()
        .find(|header| {
            header
                .name()
                .eq_ignore_ascii_case("content-security-policy")
        })
        .map(|header| header.value().to_owned())
        .expect("a policy");
    assert!(csp.contains("https://fonts.example"), "{csp}");
    assert!(csp.contains("'sha256-"), "{csp}");
}

/// A part that counts what the app asked of it.
#[derive(Clone, Default)]
struct Counted {
    installed: Arc<AtomicUsize>,
    checked: Arc<AtomicUsize>,
}

impl AppPart<()> for Counted {
    fn install(
        &self,
        served: &Served,
        router: Router<()>,
        server: ServerConfig,
    ) -> Result<(Router<()>, ServerConfig), AnyError> {
        assert_eq!(served.listed_pages(), ["/", "/mission"]);
        self.installed.fetch_add(1, Ordering::SeqCst);
        Ok((router, server))
    }

    fn check(&self, _app: &AppSettings) -> Result<(), AnyError> {
        self.checked.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// The fleet's facts, as an app publishes them on its admin listener.
struct Fleet;

impl ServiceDeclaration for Fleet {
    fn service(&self) -> &str {
        "www"
    }

    fn rendered(&self) -> BTreeMap<String, String> {
        BTreeMap::from([("replicas".to_owned(), "1".to_owned())])
    }
}

fn listen() -> fundamental_core::listen::Listen {
    settings(Environment::Development, "")
        .web_listen()
        .expect("a web address")
}

/// The app `www` in `environment`, its `web` surface given `web` beside its port.
fn settings(environment: Environment, web: &str) -> AppSettings {
    let dir = tempfile::tempdir().expect("a directory");
    let manifest = dir.path().join(AppSettings::MANIFEST);
    std::fs::write(
        &manifest,
        format!(
            "[server]\ntls = \"terminated_upstream\"\n[apps.www]\nweb = {{ host = \"127.0.0.1\", port = 18080{web} }}\nadmin = {{ host = \"127.0.0.1\", port = 18081 }}\n"
        ),
    )
    .expect("written");
    AppSettings::load_in("www", &manifest, environment).expect("the manifest loads")
}

#[test]
fn a_check_builds_every_part_and_starts_nothing() {
    let part = Counted::default();
    app()
        .admin(|_: &AppSettings| Ok::<_, AnyError>(Fleet))
        .part(part.clone())
        .check(&settings(Environment::Development, ""))
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(part.installed.load(Ordering::SeqCst), 1);
    assert_eq!(part.checked.load(Ordering::SeqCst), 1);
}

#[test]
fn an_app_serving_pages_with_no_state_is_refused() {
    let error = App::<()>::new("www")
        .pages(register_pages)
        .check(&settings(Environment::Development, ""))
        .expect_err("no state to serve pages over");
    assert_eq!(error.get().error_code(), "SERVE_002");
}

#[test]
fn a_deployed_release_naming_no_bundle_is_refused() {
    let error = app()
        .check(&settings(
            Environment::Staging,
            ", tls = \"terminated_upstream\"",
        ))
        .expect_err("a deployed release names its bundle");
    assert_eq!(error.get().error_code(), "CFG_031", "{error:?}");
}

/// A part that refuses the configuration it is checked against.
struct Refusing;

impl AppPart<()> for Refusing {
    fn install(
        &self,
        _served: &Served,
        router: Router<()>,
        server: ServerConfig,
    ) -> Result<(Router<()>, ServerConfig), AnyError> {
        Ok((router, server))
    }

    fn check(&self, _app: &AppSettings) -> Result<(), AnyError> {
        Err(ServerError::PathTaken {
            path: "/robots.txt".to_owned(),
            held_by: "a route",
        }
        .into())
    }
}

#[test]
fn a_check_fails_when_a_part_refuses() {
    let error = app()
        .part(Refusing)
        .check(&settings(Environment::Development, ""))
        .expect_err("the part refused");
    assert!(error.to_string().contains("/robots.txt"), "{error}");
}

/// A part that records whether the router it was given serves the live reload.
#[derive(Clone, Default)]
struct Watching {
    live: Arc<AtomicBool>,
}

impl AppPart<()> for Watching {
    fn install(
        &self,
        _served: &Served,
        router: Router<()>,
        server: ServerConfig,
    ) -> Result<(Router<()>, ServerConfig), AnyError> {
        self.live
            .store(router.serves_live_reload(), Ordering::SeqCst);
        Ok((router, server))
    }

    fn check(&self, _app: &AppSettings) -> Result<(), AnyError> {
        Ok(())
    }
}

#[test]
fn an_app_in_development_serves_its_bundle_from_disk_and_live_reloads() {
    let bundle = tempfile::tempdir().expect("a bundle");
    let part = Watching::default();
    app()
        .part(part.clone())
        .check(&settings(
            Environment::Development,
            &format!(", dir = \"{}\"", bundle.path().display()),
        ))
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert!(part.live.load(Ordering::SeqCst));
}

#[test]
fn the_app_macro_names_the_app_after_its_crate() {
    let app: App<()> = fundamental_app::app!();
    assert_eq!(app.name(), "fundamental_app");
}

#[test]
fn an_app_its_manifest_does_not_declare_is_refused() {
    let dir = tempfile::tempdir().expect("a directory");
    let manifest = dir.path().join(AppSettings::MANIFEST);
    std::fs::write(&manifest, "[apps.www]\nweb = { port = 18080 }\n").expect("written");
    let settings = AppSettings::load_in("renamed", &manifest, Environment::Development)
        .expect("the manifest loads");
    let error = App::<()>::new("renamed")
        .state(|_| Ok(()))
        .check(&settings)
        .expect_err("no [apps.renamed]");
    assert_eq!(error.get().error_code(), "SERVE_003");
    assert!(error.to_string().contains("[apps.renamed]"), "{error}");
}

/// The settings of `www` from a manifest holding `toml`.
fn settings_from(toml: &str) -> AppSettings {
    let dir = tempfile::tempdir().expect("a directory");
    let manifest = dir.path().join(AppSettings::MANIFEST);
    std::fs::write(&manifest, toml).expect("written");
    AppSettings::load_in("www", &manifest, Environment::Development).expect("the manifest loads")
}

#[test]
fn every_surface_the_settings_enable_runs_beside_one_admin_listener() {
    assert_eq!(
        app().surfaces(&settings(Environment::Development, "")),
        ["admin", "web"]
    );
}

#[test]
fn a_surface_the_settings_leave_out_is_never_built() {
    let part = Counted::default();
    let app = app().part(part.clone());
    let admin_only = settings(Environment::Development, "").with_surfaces(["admin"]);
    assert_eq!(app.surfaces(&admin_only), ["admin"]);
    app.check(&admin_only)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(part.installed.load(Ordering::SeqCst), 0, "no web router");
    assert_eq!(part.checked.load(Ordering::SeqCst), 1);
}

#[test]
fn a_process_given_a_surface_the_app_does_not_run_is_refused() {
    // A unit naming a surface the app does not run would otherwise start and look healthy.
    let worker_only = settings(Environment::Development, "").with_surfaces(["web", "worker"]);
    let error = app()
        .check(&worker_only)
        .expect_err("the app runs no worker");
    assert_eq!(error.get().error_code(), "SERVE_004", "{error:?}");
    assert!(error.to_string().contains("worker"), "{error}");
}

#[test]
fn a_process_given_only_surfaces_the_app_runs_is_checked() {
    let web_and_admin = settings(Environment::Development, "").with_surfaces(["web", "admin"]);
    app()
        .check(&web_and_admin)
        .unwrap_or_else(|error| panic!("{error:?}"));
}

#[test]
fn an_app_with_no_pages_binds_no_web_listener() {
    let no_web = settings_from("[apps.www]\nadmin = { port = 18081 }\n");
    let app = App::<()>::new("www");
    assert_eq!(app.surfaces(&no_web), ["admin"]);
    app.check(&no_web)
        .unwrap_or_else(|error| panic!("{error:?}"));
}

fn register_api(router: Router<()>) -> Result<Router<()>, ServerError> {
    router.route(Route::post(
        "/echo",
        |_: HttpContext<()>, input: String| async move { Ok::<_, AnyError>(input) },
    ))
}

#[tokio::test]
async fn the_api_router_answers_the_apis_routes() {
    let router = app()
        .api(register_api)
        .api_router()
        .unwrap_or_else(|error| panic!("{error:?}"));
    let response = router
        .dispatch(
            ServerRequest::new(Method::Post, "/echo", ())
                .with_headers(
                    [("content-type".to_owned(), "application/json".to_owned())]
                        .into_iter()
                        .collect(),
                )
                .with_body(b"\"hi\"".to_vec()),
        )
        .await
        .buffered()
        .await;
    assert_eq!(response.status(), 200, "{}", body(&response));
    assert_eq!(body(&response), "\"hi\"");
}

#[test]
fn an_api_runs_beside_the_pages_over_the_same_state() {
    let both = settings_from(
        "[apps.www]\nweb = { port = 18080 }\napi = { port = 18082 }\nadmin = { port = 18081 }\n",
    );
    let app = app().api(register_api);
    assert_eq!(app.surfaces(&both), ["admin", "web", "api"]);
    app.check(&both).unwrap_or_else(|error| panic!("{error:?}"));
}

#[test]
fn an_api_the_manifest_gives_no_address_is_refused() {
    let error = app()
        .api(register_api)
        .check(&settings(Environment::Development, ""))
        .expect_err("no [apps.www.api]");
    assert!(error.to_string().contains("apps.www.api"), "{error}");
}

#[test]
fn a_socket_runs_beside_the_pages_and_is_built_by_a_check() {
    let both = settings_from(
        "[apps.www]\nweb = { port = 18080 }\nsocket = { port = 18083 }\nadmin = { port = 18081 }\n",
    );
    let app = app()
        .socket(|router: WsRouter| router)
        .socket_config(WsConfig::public);
    assert_eq!(app.surfaces(&both), ["admin", "web", "socket"]);
    app.check(&both).unwrap_or_else(|error| panic!("{error:?}"));
}

#[test]
fn a_socket_the_manifest_gives_no_address_is_refused() {
    let error = app()
        .socket(|router: WsRouter| router)
        .check(&settings(Environment::Development, ""))
        .expect_err("no [apps.www.socket]");
    assert!(error.to_string().contains("apps.www.socket"), "{error}");
}

#[derive(Serialize, Deserialize, Message)]
#[subject = "events.hello"]
struct Hello {
    name: String,
}

fundamental_macros::mount_messages! { Hello }

#[fundamental_macros::subscribe]
async fn on_hello(message: ReceivedMessage<Hello>) -> NatsResult<()> {
    assert_eq!(message.payload.name, "Ada");
    Ok(())
}

fundamental_macros::mount_workers! { on_hello }

#[tokio::test]
async fn a_test_calls_a_workers_handler_directly() {
    assert_eq!(describe_workers(), ["on_hello"]);
    on_hello(
        ReceivedMessage::delivered(Hello {
            name: "Ada".to_owned(),
        })
        .expect("serializes"),
    )
    .await
    .expect("handled");
}

#[test]
fn a_worker_runs_beside_the_pages_and_a_check_opens_no_connection() {
    let both = settings_from(
        "[messaging.nats]\nurl = \"nats://127.0.0.1:1\"\n[apps.www]\nweb = { port = 18080 }\nworker = {}\nadmin = { port = 18081 }\n",
    );
    let app = app().worker(register_workers).messages(describe_messages);
    assert_eq!(app.surfaces(&both), ["admin", "web", "worker"]);
    app.check(&both).unwrap_or_else(|error| panic!("{error:?}"));
}

#[test]
fn a_worker_with_no_nats_settings_is_refused() {
    let error = app()
        .worker(register_workers)
        .check(&settings(Environment::Development, ""))
        .expect_err("no [nats]");
    assert!(error.to_string().to_lowercase().contains("nats"), "{error}");
}

fundamental_macros::mount_slices! {}
fundamental_macros::mount_projections! {}

// An unconnected pool is made on the Tokio runtime, as `<app> check-config` runs its check.
#[tokio::test]
async fn a_sync_server_runs_beside_the_pages_and_a_check_connects_nothing() {
    let both = settings_from(
        "[apps.www]\nweb = { port = 18080 }\nsync = { port = 18084 }\nadmin = { port = 18081 }\n[apps.www.database.sql]\nurl = \"postgresql://yugabyte@127.0.0.1:1/www\"\nacquire_timeout_secs = 2\n",
    );
    let app = app()
        .sync(register_slices)
        .projections(register_projections);
    assert_eq!(app.surfaces(&both), ["admin", "web", "sync"]);
    app.check(&both).unwrap_or_else(|error| panic!("{error:?}"));
}

#[test]
fn a_sync_server_with_no_database_is_refused() {
    let no_database = settings_from(
        "[apps.www]\nweb = { port = 18080 }\nsync = { port = 18084 }\nadmin = { port = 18081 }\n",
    );
    let error = app()
        .sync(register_slices)
        .check(&no_database)
        .expect_err("no pool to sync from");
    assert!(error.to_string().contains("default"), "{error}");
}

#[test]
fn a_sync_server_over_a_keyspace_is_built_by_a_check() {
    let cql = settings_from(
        "[apps.www]\nsync = { port = 18084 }\nadmin = { port = 18081 }\n[apps.www.database.cql]\nhosts = [\"127.0.0.1:1\"]\nkeyspace = \"www\"\n",
    );
    let app = App::<()>::new("www").sync(register_slices);
    assert_eq!(app.surfaces(&cql), ["admin", "sync"]);
    app.check(&cql).unwrap_or_else(|error| panic!("{error:?}"));
}

/// `www`'s settings with a sync surface over the databases `databases` declares.
fn sync_settings(databases: &str) -> AppSettings {
    settings_from(&format!(
        "[apps.www]\nsync = {{ port = 18084 }}\nadmin = {{ port = 18081 }}\n{databases}"
    ))
}

const SQL: &str = "[apps.www.database.sql]\nurl = \"postgresql://yugabyte@127.0.0.1:1/www\"\nacquire_timeout_secs = 2\n";
const CQL: &str = "[apps.www.database.cql]\nhosts = [\"127.0.0.1:1\"]\nkeyspace = \"www\"\n";

#[tokio::test]
async fn a_sync_server_told_its_keyspace_syncs_over_it_beside_a_sql_database() {
    App::<()>::new("www")
        .sync(register_slices)
        .sync_store(SyncStore::Cql)
        .check(&sync_settings(&format!("{SQL}{CQL}")))
        .unwrap_or_else(|error| panic!("{error:?}"));
}

#[tokio::test]
async fn a_sync_server_told_a_store_the_manifest_does_not_declare_is_refused() {
    let over_cql = App::<()>::new("www")
        .sync(register_slices)
        .sync_store(SyncStore::Cql)
        .check(&sync_settings(SQL))
        .expect_err("no keyspace to sync over");
    assert_eq!(over_cql.get().error_code(), "SERVE_005", "{over_cql:?}");
    assert!(over_cql.to_string().contains("cql"), "{over_cql}");
    let over_sql = App::<()>::new("www")
        .sync(register_slices)
        .sync_store(SyncStore::Sql)
        .check(&sync_settings(CQL))
        .expect_err("no SQL database to sync over");
    assert_eq!(over_sql.get().error_code(), "SERVE_005", "{over_sql:?}");
}

#[test]
fn what_a_worker_is_provided_is_built_when_it_starts_not_when_checked() {
    let both = settings_from(
        "[messaging.nats]\nurl = \"nats://127.0.0.1:1\"\n[apps.www]\nworker = {}\nadmin = { port = 18081 }\n",
    );
    let built = Arc::new(AtomicUsize::new(0));
    let counted = built.clone();
    App::<()>::new("www")
        .worker(register_workers)
        .provide_to_workers(move |_served: &Served| {
            counted.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, AnyError>(18_u8) }
        })
        .check(&both)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(built.load(Ordering::SeqCst), 0);
}
