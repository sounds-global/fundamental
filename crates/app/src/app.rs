use std::process::ExitCode;
use std::sync::Arc;

use fundamental_api::ApiServer;
use fundamental_config::AppSettings;
use fundamental_core::address::AddressSpace;
use fundamental_core::listen::Listen;
use fundamental_core::{AnyError, ServiceDeclaration, ServiceState};
use fundamental_db::PoolProbe;
use fundamental_i18n::Catalog;
use fundamental_observability::{ObservabilityBuilder, check_release};
use fundamental_runtime::{AdminSurface, Runtime};
use fundamental_server::{
    ActionDescriptor, CspSource, Indexing, LocaleChoice, LocalePolicy, PreferencePolicy, Router,
    SecurityHeaders, ServerConfig, StaticSource,
};
use fundamental_sql::SqlConnections;
use fundamental_web::{Document, SiteIcon, WebServer, hydration_script_csp_hash};
use tracing::{error, info, instrument};

use crate::declared::Declared;
use crate::launch::Launch;
use crate::part_surface::PartSurface;
use crate::unlisted::Unlisted;
use crate::{AppError, AppPart, Served};

const ADMIN: &str = "admin";
const WEB: &str = "web";
const API: &str = "api";
/// Every surface an app may run, in the order it names them.
const SURFACES: [&str; 6] = [ADMIN, WEB, API, "worker", "socket", "sync"];

type Build<T> = Box<dyn Fn() -> Result<T, AnyError> + Send + Sync>;
type Routes<S> = Box<dyn Fn(Router<S>) -> Result<Router<S>, AnyError> + Send + Sync>;
type State<S> = Box<dyn Fn(&Served) -> Result<S, AnyError> + Send + Sync>;
type Admin = Box<dyn Fn(&AppSettings) -> Result<Declared, AnyError> + Send + Sync>;

/// One app as one value, served by [`run`](Self::run) and answered in tests by [`router`](Self::router).
///
/// ```
/// # extern crate fundamental_app;
/// # extern crate fundamental_server;
/// use fundamental_server::{HttpContext, Route, Router, ServerError};
/// use fundamental_app::App;
///
/// fn register_pages(router: Router<()>) -> Result<Router<()>, ServerError> {
///     router.route(Route::html("/", |_: HttpContext<()>| async { Ok("<h1>Hi</h1>".to_owned()) }))
/// }
///
/// let app = App::new("www").pages(register_pages).state(|_| Ok(()));
/// assert_eq!(app.router()?.page_paths(), ["/"]);
/// # Ok::<(), fundamental_core::AnyError>(())
/// ```
#[must_use]
pub struct App<S> {
    name: &'static str,
    version: &'static str,
    built: &'static str,
    catalog: Option<Build<Catalog>>,
    languages: Option<LocalePolicy>,
    appearance: Option<PreferencePolicy>,
    addresses: Option<Build<AddressSpace>>,
    pages: Option<Routes<S>>,
    api: Option<Routes<S>>,
    look: Option<Build<()>>,
    icons: &'static [SiteIcon],
    document: Document,
    font_sources: Vec<CspSource>,
    image_sources: Vec<CspSource>,
    indexing: Indexing,
    unlisted: Vec<&'static str>,
    state: Option<State<S>>,
    admin: Option<Admin>,
    parts: Vec<Box<dyn AppPart<S>>>,
    actions: Option<fn() -> Vec<ActionDescriptor>>,
    #[cfg(feature = "socket")]
    socket: crate::socket::Socket,
    #[cfg(feature = "worker")]
    workers: crate::worker::Workers,
    #[cfg(feature = "sync")]
    sync: crate::sync::LiveSync,
}

impl<S> App<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// The app `name`, as `[apps.<name>]` configures it, serving nothing yet.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            version: "0.0.0",
            built: "dist",
            catalog: None,
            languages: None,
            appearance: None,
            addresses: None,
            pages: None,
            api: None,
            look: None,
            icons: &[],
            document: Document::new(),
            font_sources: Vec::new(),
            image_sources: Vec::new(),
            indexing: Indexing::Public,
            unlisted: Vec::new(),
            state: None,
            admin: None,
            parts: Vec::new(),
            actions: None,
            #[cfg(feature = "socket")]
            socket: crate::socket::Socket::default(),
            #[cfg(feature = "worker")]
            workers: crate::worker::Workers::default(),
            #[cfg(feature = "sync")]
            sync: crate::sync::LiveSync::default(),
        }
    }

    /// The app's version, as the dev tools name it.
    pub fn version(mut self, version: &'static str) -> Self {
        self.version = version;
        self
    }

    /// Where the build writes the bundle on a developer's machine.
    pub fn built(mut self, dir: &'static str) -> Self {
        self.built = dir;
        self
    }

    /// Every word the app shows, in every language, installed once when the app is served.
    pub fn catalog<E>(mut self, catalog: fn() -> Result<Catalog, E>) -> Self
    where
        E: Into<AnyError> + 'static,
    {
        self.catalog = Some(Box::new(move || catalog().map_err(Into::into)));
        self
    }

    /// The languages the app answers in and how a reader's is chosen.
    pub fn languages(mut self, policy: LocalePolicy) -> Self {
        self.languages = Some(policy);
        self
    }

    /// The reader's theme, written as `<html data-theme>` before the first paint.
    pub fn appearance(mut self, policy: PreferencePolicy) -> Self {
        self.appearance = Some(policy);
        self
    }

    /// Where each page lives in each language: the generated `address_space`.
    pub fn addresses<E>(mut self, addresses: fn() -> Result<AddressSpace, E>) -> Self
    where
        E: Into<AnyError> + 'static,
    {
        self.addresses = Some(Box::new(move || addresses().map_err(Into::into)));
        self
    }

    /// Adds the app's pages and actions.
    pub fn pages<F, E>(mut self, pages: F) -> Self
    where
        F: Fn(Router<S>) -> Result<Router<S>, E> + Send + Sync + 'static,
        E: Into<AnyError> + 'static,
    {
        self.pages = Some(Box::new(move |router| pages(router).map_err(Into::into)));
        self
    }

    /// Adds the app's HTTP API, served by its own listener at `[apps.<name>.api]`.
    pub fn api<F, E>(mut self, register: F) -> Self
    where
        F: Fn(Router<S>) -> Result<Router<S>, E> + Send + Sync + 'static,
        E: Into<AnyError> + 'static,
    {
        self.api = Some(Box::new(move |router| register(router).map_err(Into::into)));
        self
    }

    /// Adds the app's WebSocket server, served at `[apps.<name>.socket]`.
    #[cfg(feature = "socket")]
    pub fn socket<F>(mut self, register: F) -> Self
    where
        F: Fn(fundamental_ws::WsRouter) -> fundamental_ws::WsRouter + Send + Sync + 'static,
    {
        self.socket.routes(Box::new(register));
        self
    }

    /// Tunes the WebSocket server's settings beyond the address the manifest gives.
    #[cfg(feature = "socket")]
    pub fn socket_config<F>(mut self, tune: F) -> Self
    where
        F: Fn(fundamental_ws::WsConfig) -> fundamental_ws::WsConfig + Send + Sync + 'static,
    {
        self.socket.tune(Box::new(tune));
        self
    }

    /// Adds the app's worker; it connects to NATS when it starts, never when built or checked.
    #[cfg(feature = "worker")]
    pub fn worker<F>(mut self, register: F) -> Self
    where
        F: for<'a> Fn(&'a fundamental_nats::WorkerContext) -> fundamental_nats::WorkersFuture<'a>
            + Send
            + Sync
            + 'static,
    {
        self.workers.register(std::sync::Arc::new(register));
        self
    }

    /// Gives every one of the worker's handlers the value `provide` builds when the worker starts.
    #[cfg(feature = "worker")]
    pub fn provide_to_workers<T, F, Fut, E>(mut self, provide: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&Served) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
        E: Into<AnyError> + 'static,
    {
        self.workers.provide(provide);
        self
    }

    /// The messages the app's worker handles, as the dev tools describe them.
    #[cfg(feature = "worker")]
    pub fn messages(mut self, describe: fn() -> Vec<fundamental_nats::MessageDescriptor>) -> Self {
        self.workers.messages(describe);
        self
    }

    /// Adds the app's live slice-sync server at `[apps.<name>.sync]`.
    #[cfg(feature = "sync")]
    pub fn sync(
        mut self,
        register: fn(
            fundamental_sync_server::SliceRegistry,
        ) -> fundamental_sync_server::SliceRegistry,
    ) -> Self {
        self.sync.slices(register);
        self
    }

    /// The store the sync server reads and writes its slices in, when the app declares more than one.
    #[cfg(feature = "sync")]
    pub fn sync_store(mut self, store: crate::SyncStore) -> Self {
        self.sync.store(store);
        self
    }

    /// The projections the sync server keeps fresh.
    #[cfg(feature = "sync")]
    pub fn projections(
        mut self,
        register: fn(
            fundamental_sync_server::ProjectionRegistry,
        ) -> fundamental_sync_server::ProjectionRegistry,
    ) -> Self {
        self.sync.projections(register);
        self
    }

    /// Installs the app's look, run each time the router is built, before any page is registered.
    pub fn look<E>(mut self, look: fn() -> Result<(), E>) -> Self
    where
        E: Into<AnyError> + 'static,
    {
        self.look = Some(Box::new(move || look().map_err(Into::into)));
        self
    }

    /// The app's icons, each served at its path and linked from every page's head.
    pub fn icons(mut self, icons: &'static [SiteIcon]) -> Self {
        self.icons = icons;
        self
    }

    /// The shell every page wears: its style links and its share card.
    pub fn document(mut self, document: Document) -> Self {
        self.document = document;
        self
    }

    /// Lets every page load fonts from `source` as well as from the app itself.
    pub fn font_source(mut self, source: CspSource) -> Self {
        self.font_sources.push(source);
        self
    }

    /// Lets every page load images from `source` as well as from the app itself.
    pub fn image_source(mut self, source: CspSource) -> Self {
        self.image_sources.push(source);
        self
    }

    /// Whether a search engine may index the app at all.
    pub fn indexing(mut self, indexing: Indexing) -> Self {
        self.indexing = indexing;
        self
    }

    /// Keeps the pages at and below `prefix` out of search, answered with `X-Robots-Tag: noindex`.
    pub fn unlisted(mut self, prefix: &'static str) -> Self {
        self.unlisted.push(prefix);
        self
    }

    /// The state every page is served over, built from what the app is [served with](Served).
    pub fn state<F>(mut self, state: F) -> Self
    where
        F: Fn(&Served) -> Result<S, AnyError> + Send + Sync + 'static,
    {
        self.state = Some(Box::new(state));
        self
    }

    /// What the admin listener publishes about the process, built from the app's configuration.
    pub fn admin<F, D, E>(mut self, declaration: F) -> Self
    where
        F: Fn(&AppSettings) -> Result<D, E> + Send + Sync + 'static,
        D: ServiceDeclaration,
        E: Into<AnyError> + 'static,
    {
        self.admin = Some(Box::new(move |settings| {
            declaration(settings)
                .map(|declared| Declared::new(Box::new(declared)))
                .map_err(Into::into)
        }));
        self
    }

    /// Adds `part`, installed after the app's own pages in the order parts are added.
    pub fn part(mut self, part: impl AppPart<S>) -> Self {
        self.parts.push(Box::new(part));
        self
    }

    /// The app's actions, as the dev tools describe them: the generated `describe_actions`.
    pub fn actions(mut self, actions: fn() -> Vec<ActionDescriptor>) -> Self {
        self.actions = Some(actions);
        self
    }

    /// The app's name: its key in `[apps.<name>]`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The router the app serves, short of what its parts add.
    ///
    /// # Errors
    /// The look, the address space or a page refused, an icon's path already served, or a
    /// language policy that reads no cookie ([`AppError::ChoiceUnremembered`]).
    pub fn router(&self) -> Result<Router<S>, AnyError> {
        self.build_router(false)
    }

    /// The router the API listener serves; empty when the app declares no [`api`](Self::api).
    ///
    /// # Errors
    /// A route the API's registration refused.
    pub fn api_router(&self) -> Result<Router<S>, AnyError> {
        match &self.api {
            Some(api) => api(Router::new()),
            None => Ok(Router::new()),
        }
    }

    /// The app's own pages a search engine may list, at their canonical paths.
    ///
    /// # Errors
    /// As [`router`](Self::router).
    pub fn listed_pages(&self) -> Result<Vec<String>, AnyError> {
        Ok(self.listed(&self.router()?))
    }

    /// The web listener's settings, listening at `listen`.
    ///
    /// # Errors
    /// The hydration script's recorded hash is malformed.
    pub fn server_config(&self, listen: Listen) -> Result<ServerConfig, AnyError> {
        let headers = self.font_sources.iter().cloned().fold(
            SecurityHeaders::web()
                .with_script_hash(hydration_script_csp_hash()?)
                .with_indexing(self.indexing),
            SecurityHeaders::with_font_source,
        );
        let headers = self
            .image_sources
            .iter()
            .cloned()
            .fold(headers, SecurityHeaders::with_image_source);
        let mut config = ServerConfig::listening(listen)
            .with_allow_dev_tools_cors(cfg!(feature = "introspect"))
            .with_security_headers(headers);
        if let Some(policy) = &self.languages {
            config = config.with_locale_policy(policy.clone());
        }
        if let Some(policy) = &self.appearance {
            config = config.with_preference_policy(policy.clone());
        }
        Ok(config)
    }

    /// Whether the app starts as `settings` configure it, short of listening; starts nothing.
    ///
    /// # Errors
    /// Whatever the app would refuse at start-up.
    #[instrument(skip_all, fields(app = self.name, environment = %settings.environment()))]
    #[expect(
        clippy::cognitive_complexity,
        reason = "the span's own expansion; the body is six steps"
    )]
    pub fn check(&self, settings: &AppSettings) -> Result<(), AnyError> {
        declared(settings)?;
        self.runs_every_named(settings)?;
        self.build_catalog()?;
        let connections =
            SqlConnections::unconnected(&settings.config().database, settings.environment())?;
        drop(self.assemble(settings, connections, None)?);
        self.parts
            .iter()
            .try_for_each(|part| part.check(settings))?;
        info!(surfaces = ?self.surfaces(settings), "the app starts as configured");
        Ok(())
    }

    /// The app's catalog, built; `None` when it declares none.
    fn build_catalog(&self) -> Result<Option<Catalog>, AnyError> {
        self.catalog.as_ref().map(|catalog| catalog()).transpose()
    }

    /// Serves the app until the process is told to stop.
    ///
    /// # Errors
    /// The configuration, the telemetry, the catalog, a database or anything
    /// [`check`](Self::check) refuses; a surface that fails while running.
    pub async fn serve(self) -> Result<(), AnyError> {
        let (settings, telemetry) = ObservabilityBuilder::boot(self.name)?;
        #[cfg(feature = "introspect")]
        let drain = fundamental_introspect::Drain::new();
        #[cfg(feature = "introspect")]
        let telemetry = telemetry.with_layer(drain.clone());
        let _telemetry = telemetry.init()?;
        #[cfg(feature = "introspect")]
        let outcome = self.serve_with(&settings, Some(drain)).await;
        #[cfg(not(feature = "introspect"))]
        let outcome = self.serve_with(&settings, None).await;
        // Logged here, while the telemetry it reaches the collector through is still installed.
        if let Err(failure) = &outcome {
            stopped(self.name, failure);
        }
        outcome
    }

    /// Serves the app as `settings` configure it, its telemetry installed.
    async fn serve_with(
        &self,
        settings: &AppSettings,
        #[cfg(feature = "introspect")] drain: Option<fundamental_introspect::Drain>,
        #[cfg(not(feature = "introspect"))] drain: Option<()>,
    ) -> Result<(), AnyError> {
        declared(settings)?;
        self.runs_every_named(settings)?;
        if let Some(catalog) = self.build_catalog()? {
            fundamental_i18n::install_catalog(catalog);
        }
        let connections = SqlConnections::from_settings(
            &settings.config().database,
            settings.environment(),
            None,
        )
        .await?;
        let runtime = self.assemble(settings, connections, drain)?;
        info!(
            app = settings.name(),
            environment = %settings.environment(),
            surfaces = ?self.surfaces(settings),
            "serving the app"
        );
        runtime.run().await?;
        Ok(())
    }

    /// Runs the binary: `check-config [manifest]` checks a release, anything else serves the app.
    pub fn run(self) -> ExitCode {
        let launch = Launch::from_args(std::env::args_os().skip(1));
        let name = self.name;
        let outcome = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(self.launch(launch)),
            Err(problem) => {
                eprintln!("{name}: the async runtime could not start: {problem}");
                return ExitCode::FAILURE;
            }
        };
        match outcome {
            Ok(()) => ExitCode::SUCCESS,
            Err(failure) => {
                let code = failure.get().error_code();
                // Also where no subscriber could be installed: a configuration that did not load.
                eprintln!("{name}: {code}: {failure}");
                ExitCode::from(failure_status(failure.get().exit_code()))
            }
        }
    }

    async fn launch(self, launch: Launch) -> Result<(), AnyError> {
        match launch {
            Launch::Serve => self.serve().await,
            Launch::CheckConfig(manifest) => {
                // The release check keeps its bootstrap subscriber for the whole process.
                let outcome = check_release(self.name, manifest)
                    .map_err(AnyError::from)
                    .and_then(|settings| self.check(&settings));
                if let Err(failure) = &outcome {
                    stopped(self.name, failure);
                }
                outcome
            }
        }
    }

    /// The router, the language cookie sent over plain `http` when `development`.
    fn build_router(&self, development: bool) -> Result<Router<S>, AnyError> {
        if let Some(look) = &self.look {
            look()?;
        }
        let mut router = Router::<S>::new();
        if let Some(pages) = &self.pages {
            router = pages(router)?;
        }
        router = self.shell().install(router)?;
        if let Some(addresses) = &self.addresses {
            router = router.with_addresses(Arc::new(addresses()?));
        }
        if let Some(policy) = &self.languages {
            let choice = LocaleChoice::of(policy, development)
                .ok_or(AppError::ChoiceUnremembered)?
                .with_arrivals();
            router = router.middleware(choice);
        }
        if !self.unlisted.is_empty() {
            router = router.middleware(Unlisted::new(self.unlisted.clone()));
        }
        Ok(router)
    }

    fn shell(&self) -> Document {
        let mut shell = self.document.clone().icons(self.icons);
        if let Some(policy) = &self.appearance {
            shell = shell.theme(policy.name());
        }
        if let Some(policy) = &self.languages {
            shell = shell.locales(
                policy
                    .supported()
                    .iter()
                    .map(|locale| policy.regional(locale))
                    .collect(),
            );
        }
        if !shell.has_bundle() {
            shell = shell.bundle(format!("/_fundamental/{}.js", self.name.replace('-', "_")));
        }
        shell
    }

    /// The pages `router` serves that a search engine may list.
    fn listed(&self, router: &Router<S>) -> Vec<String> {
        if self.indexing == Indexing::Private {
            return Vec::new();
        }
        let unlisted = Unlisted::new(self.unlisted.clone());
        router
            .page_paths()
            .into_iter()
            .filter(|path| !unlisted.covers(path))
            .map(str::to_owned)
            .collect()
    }

    /// The surfaces this process runs as `settings` configure it, by name.
    #[must_use]
    pub fn surfaces(&self, settings: &AppSettings) -> Vec<&'static str> {
        SURFACES
            .into_iter()
            .filter(|surface| self.runs(settings, surface))
            .collect()
    }

    /// Refuses a surface this process was told to run that the app does not run.
    fn runs_every_named(&self, settings: &AppSettings) -> Result<(), AppError> {
        let named = settings.named_surfaces().unwrap_or_default();
        match named.iter().find(|surface| !self.runs(settings, surface)) {
            Some(surface) => Err(AppError::SurfaceNotRun {
                app: settings.name().to_owned(),
                surface: surface.clone(),
            }),
            None => Ok(()),
        }
    }

    fn runs(&self, settings: &AppSettings, surface: &str) -> bool {
        let declared = match surface {
            ADMIN => settings.declares_surface(ADMIN),
            WEB => self.pages.is_some(),
            API => self.api.is_some(),
            #[cfg(feature = "socket")]
            crate::socket::SOCKET => self.socket.declared(),
            #[cfg(feature = "worker")]
            crate::worker::WORKER => self.workers.declared(),
            #[cfg(feature = "sync")]
            crate::sync::SYNC => self.sync.declared(),
            _ => false,
        };
        declared && settings.surface_enabled(surface)
    }

    /// Every surface the app runs as `settings` configure it, over `connections`.
    fn assemble(
        &self,
        settings: &AppSettings,
        connections: SqlConnections,
        #[cfg(feature = "introspect")] drain: Option<fundamental_introspect::Drain>,
        #[cfg(not(feature = "introspect"))] _drain: Option<()>,
    ) -> Result<Runtime, AnyError> {
        #[cfg(feature = "introspect")]
        let mut drain = drain;
        let web = self.runs(settings, WEB);
        let api = self.runs(settings, API);
        let router = if web {
            let router = self.bundled_router(settings)?;
            #[cfg(feature = "introspect")]
            let router = self.introspection(drain.take()).mount(router)?;
            Some(router)
        } else {
            None
        };
        let listed = router
            .as_ref()
            .map(|router| self.listed(router))
            .unwrap_or_default();
        let served = Served::new(settings.clone(), connections, listed);
        let mut runtime = Runtime::new();
        if self.runs(settings, ADMIN) {
            runtime = runtime.with_admin(self.admin_surface(settings, &served)?)?;
        }
        let state = if web || api {
            Some(self.build_state(&served)?)
        } else {
            None
        };
        if let (Some(router), Some(state)) = (router, state.clone()) {
            let (mut router, mut server) = (router, self.server_config(settings.web_listen()?)?);
            for part in &self.parts {
                (router, server) = part.install(&served, router, server)?;
            }
            runtime = runtime.with(WebServer::new(server, state, router));
        }
        if let (true, Some(state)) = (api, state) {
            // The dev tools ride the web listener when there is one, else this one.
            let router = self.api_router()?;
            #[cfg(feature = "introspect")]
            let router = match drain.take() {
                Some(drain) if !web => self.introspection(Some(drain)).mount(router)?,
                _ => router,
            };
            let config = ServerConfig::listening(settings.surface_listen(API)?)
                .with_allow_dev_tools_cors(cfg!(feature = "introspect"));
            runtime = runtime.with(ApiServer::new(config, state, router));
        }
        runtime = self.with_background(runtime, settings, &served)?;
        for part in &self.parts {
            for surface in part.surfaces(&served)? {
                runtime = runtime.with(PartSurface::new(surface));
            }
        }
        Ok(runtime)
    }

    /// `runtime` with the socket, the worker and the sync server, each that `settings` enable.
    #[cfg_attr(
        not(any(feature = "socket", feature = "worker", feature = "sync")),
        expect(
            clippy::unused_self,
            clippy::unnecessary_wraps,
            reason = "each surface's feature adds its own step"
        )
    )]
    fn with_background(
        &self,
        runtime: Runtime,
        settings: &AppSettings,
        served: &Served,
    ) -> Result<Runtime, AnyError> {
        let _ = (settings, served);
        #[cfg(any(feature = "socket", feature = "worker", feature = "sync"))]
        let mut runtime = runtime;
        #[cfg(feature = "socket")]
        if self.runs(settings, crate::socket::SOCKET) {
            runtime = runtime.with(self.socket.server(settings)?);
        }
        #[cfg(feature = "worker")]
        if self.runs(settings, crate::worker::WORKER)
            && let Some(worker) = self.workers.surface(settings, served)?
        {
            runtime = runtime.with(worker);
        }
        #[cfg(feature = "sync")]
        if self.runs(settings, crate::sync::SYNC)
            && let Some(sync) = self.sync.surface(settings, served)?
        {
            runtime = runtime.with(sync);
        }
        Ok(runtime)
    }

    /// The router the web listener serves, with the bundle beside it.
    fn bundled_router(&self, settings: &AppSettings) -> Result<Router<S>, AnyError> {
        let development = settings.environment().is_development();
        Ok(self
            .build_router(development)?
            .static_dir_from(
                "/_fundamental",
                settings.web_bundle_dir(self.built)?,
                StaticSource::for_development(development),
            )?
            .live_reload_when(development)?)
    }

    fn build_state(&self, served: &Served) -> Result<S, AnyError> {
        self.state.as_ref().ok_or(AppError::Stateless)?(served)
    }

    /// The admin listener, probing the default pool when there is one.
    fn admin_surface(
        &self,
        settings: &AppSettings,
        served: &Served,
    ) -> Result<AdminSurface, AnyError> {
        let declared = match &self.admin {
            Some(admin) => admin(settings)?,
            None => Declared::named(settings.name()),
        };
        let mut surface = AdminSurface::from_settings(settings, ServiceState::new(), declared)?;
        if let Ok(pool) = served.pool("default") {
            surface = surface.probe(PoolProbe::new("sql", pool));
        }
        Ok(surface)
    }

    /// The dev tools' view of the app: its actions and the drain of its logs and traces.
    #[cfg(feature = "introspect")]
    fn introspection(
        &self,
        drain: Option<fundamental_introspect::Drain>,
    ) -> fundamental_introspect::Introspection {
        let introspection = fundamental_introspect::Introspection::new(self.name, self.version)
            .actions(self.actions.map(|actions| actions()).unwrap_or_default());
        #[cfg(feature = "worker")]
        let introspection = introspection.messages(self.workers.described());
        match drain {
            Some(drain) => introspection.drain(drain),
            None => introspection,
        }
    }
}

/// Refuses `settings` when their manifest declares no section for the app they were loaded for.
fn declared(settings: &AppSettings) -> Result<(), AppError> {
    if settings.config().apps.apps.contains_key(settings.name()) {
        Ok(())
    } else {
        Err(AppError::Undeclared {
            app: settings.name().to_owned(),
        })
    }
}

/// The exit status for exit code `code`, never `0` for a failure.
fn failure_status(code: i32) -> u8 {
    u8::try_from(code)
        .ok()
        .filter(|code| *code != 0)
        .unwrap_or(1)
}

/// Logs that the app `name` stopped on `failure`, with its code and whole chain.
fn stopped(name: &str, failure: &AnyError) {
    error!(
        app = name,
        error_code = failure.get().error_code(),
        error = ?failure,
        "the app stopped"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_never_exits_as_a_success() {
        assert_eq!(failure_status(78), 78);
        assert_eq!(failure_status(0), 1);
        assert_eq!(failure_status(256), 1);
        assert_eq!(failure_status(-1), 1);
    }
}
