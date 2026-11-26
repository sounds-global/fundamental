use crate::action_dispatcher::ActionRoutes;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use fundamental_core::address::{AddressSpace, ResolvedAddress, WebOrigin};
use fundamental_core::asset::AssetAddresses;
use fundamental_core::{AnyError, Error};
use fundamental_observability::trace_context::TraceParent;
use tracing::{Instrument, debug, warn};

use crate::address_stylesheet::address_stylesheet;
use crate::audience::Audience;
use crate::error_page::ErrorPage;
use crate::http_context::HttpContext;
use crate::if_none_match::if_none_match;
use crate::live_reload::{LIVE_PATH, LIVE_SCRIPT, LIVE_SCRIPT_PATH, LiveReload, LiveReloadMounted};
use crate::middleware::{ErasedMiddleware, Middleware};
use crate::route::Route;
use crate::route_pattern::RoutePattern;
use crate::segment_ledger::SegmentLedger;
use crate::segment_resolver::{ErasedSegmentResolver, SegmentResolver};
use crate::segment_stage::SegmentStage;
use crate::server_action::ServerAction;
use crate::static_asset::StaticAsset;
use crate::static_source::StaticSource;
use crate::{
    BodyPolicy, Method, PathParams, PathShape, Redirect, ServerError, ServerRequest,
    ServerResponse, ServerResult, StreamedResponse,
};

/// Chrome DevTools probes this while debugging; its absence is an ordinary 404, not a warning.
const CHROME_DEVTOOLS_WORKSPACE_PATH: &str = "/.well-known/appspecific/com.chrome.devtools.json";

/// Routes a request to a handler by method and path, then runs it.
///
/// Two routes for the same method and path are a [`ServerError::RouteConflict`].
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
pub struct Router<S> {
    routes: Vec<Arc<Route<S>>>,
    /// Routes addressable by action marker during a Leptos SSR render.
    action_routes: Arc<ActionRoutes<S>>,
    middleware: Vec<Box<dyn ErasedMiddleware<S>>>,
    /// Whether a router-level middleware declared [`Middleware::NEEDS_PRINCIPAL`], gating every route.
    needs_principal: bool,
    /// Whether a router-level middleware declared [`Middleware::GRANTS_AUTHORIZATION`].
    grants_authorization: bool,
    /// Whether that router-level layer takes its object from the path.
    authorizes_from_path: bool,
    /// Whether a router-level middleware declared [`Middleware::READS_BODY`].
    reads_body: bool,
    assets: HashMap<String, StaticAsset>,
    /// Documents published at exact paths, each for every origin (`None`) or one; consulted before `assets`.
    documents: HashMap<String, Vec<(Option<WebOrigin>, StaticAsset)>>,
    /// Disk-served mounts, as (url prefix, canonicalised root).
    disk_dirs: Vec<(String, PathBuf)>,
    /// Cache policies for static assets, as (url prefix, `Cache-Control`).
    static_cache: Vec<(String, &'static str)>,
    /// The version every asset and every-origin document is linked by, by path.
    asset_addresses: Arc<AssetAddresses>,
    /// The paths, and directories ending in `/`, answered before admission.
    before_admission: Vec<String>,
    /// The page that answers an unmatched request from a browser; never matched by path.
    not_found: Option<Route<S>>,
    /// The page that answers a navigation that ends in an error; never matched by path.
    error_page: Option<ErrorPage<S>>,
    /// The page that answers a navigation refused with `401` or `403`; never matched by path.
    gate: Option<Route<S>>,
    /// How a request path maps onto this surface's addresses.
    addresses: Option<Arc<AddressSpace>>,
    /// What every handler is handed beside its state.
    extensions: crate::extensions::Extensions,
    /// One resolver per path parameter — see [`segment`](Self::segment).
    segments: Vec<Arc<dyn ErasedSegmentResolver>>,
}

impl<S> Default for Router<S> {
    fn default() -> Self {
        Self {
            routes: Vec::new(),
            action_routes: Arc::new(ActionRoutes::default()),
            middleware: Vec::new(),
            needs_principal: false,
            grants_authorization: false,
            authorizes_from_path: false,
            reads_body: false,
            assets: HashMap::new(),
            documents: HashMap::new(),
            disk_dirs: Vec::new(),
            static_cache: Vec::new(),
            asset_addresses: Arc::new(AssetAddresses::default()),
            before_admission: Vec::new(),
            not_found: None,
            error_page: None,
            gate: None,
            addresses: None,
            extensions: crate::extensions::Extensions::default(),
            segments: Vec::new(),
        }
    }
}

impl<S> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Registers the page that answers a person's navigation when it ends in an error.
    ///
    /// Never routed; it renders with the error's
    /// [`ShownError`](fundamental_core::error::ShownError) on its context and none of the
    /// router's middleware.
    #[must_use]
    pub fn error_page(mut self, route: Route<S>) -> Self {
        self.error_page = Some(ErrorPage {
            route,
            request_for: ServerRequest::for_error_page,
        });
        self
    }
}

impl<S> Router<S>
where
    S: Send + Sync + 'static,
{
    /// Creates an empty router.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `route`.
    ///
    /// # Errors
    /// Returns [`ServerError::RouteConflict`] if a route with the same method and
    /// path template is already registered, and [`ServerError::PathTaken`] if it is a
    /// `GET` at a path a [`document`](Self::document) holds.
    pub fn route(self, route: Route<S>) -> ServerResult<Self> {
        self.route_arc(Arc::new(route))
    }

    fn route_arc(mut self, route: Arc<Route<S>>) -> ServerResult<Self> {
        if route.method() == Method::Get && self.documents.contains_key(route.pattern().raw()) {
            return Err(ServerError::PathTaken {
                path: route.pattern().raw().to_owned(),
                held_by: "a route",
            });
        }
        if let Some(existing) = self.routes.iter().find(|existing| {
            existing.method() == route.method() && existing.pattern().same_shape(route.pattern())
        }) {
            if existing.pattern().raw() == route.pattern().raw() {
                return Err(ServerError::RouteConflict {
                    method: route.method().to_string(),
                    path: route.pattern().raw().to_owned(),
                });
            }
            return Err(ServerError::RouteAmbiguous {
                method: route.method().to_string(),
                path: route.pattern().raw().to_owned(),
                existing: existing.pattern().raw().to_owned(),
            });
        }
        // The most specific template wins whatever the registration order; equal ranks keep it.
        let key = route.pattern().specificity();
        let at = self
            .routes
            .partition_point(|existing| existing.pattern().specificity() <= key);
        self.routes.insert(at, route);
        Ok(self)
    }

    /// Registers an action route for HTTP and request-scoped async SSR calls.
    ///
    /// # Errors
    /// As [`route`](Self::route): [`ServerError::RouteConflict`] when the action's method and
    /// path template are taken, [`ServerError::PathTaken`] when a document holds its `GET` path.
    pub fn action<M>(self) -> ServerResult<Self>
    where
        M: ServerAction<S> + 'static,
    {
        let route = Arc::new(M::route());
        let dispatch_route = Arc::clone(&route);
        self.action_routes
            .insert::<M>(move |context, call, segments| {
                let route = Arc::clone(&dispatch_route);
                Box::pin(async move { route.dispatch_action(context, call, &segments).await })
            });
        self.route_arc(route)
    }

    /// Declares how path parameter `resolver.param()` is resolved from its public spelling to the record's id.
    ///
    /// A non-canonical spelling earns one `301` only once authorization affirmed every record.
    ///
    /// # Errors
    /// [`ServerError::SegmentsMisdeclared`] when the parameter is already declared.
    pub fn segment<R: SegmentResolver>(mut self, resolver: R) -> ServerResult<Self> {
        if self
            .segments
            .iter()
            .any(|declared| declared.param() == SegmentResolver::param(&resolver))
        {
            return Err(ServerError::SegmentsMisdeclared {
                problems: format!("`{}` is declared twice", SegmentResolver::param(&resolver)),
            });
        }
        let resolver: Arc<dyn ErasedSegmentResolver> = Arc::new(resolver);
        // An action a page dispatches in process resolves its segments as an HTTP call does.
        self.action_routes.declare_segment(Arc::clone(&resolver));
        self.segments.push(resolver);
        Ok(self)
    }

    /// Keeps `old` answering after its pages moved to `new`, with one `301`.
    ///
    /// # Errors
    /// [`ServerError::SegmentsMisdeclared`] when `new` captures a parameter `old` does not,
    /// and the errors of [`route`](Self::route) for `old`.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn retire(self, old: &str, new: &str) -> ServerResult<Self> {
        let from = RoutePattern::parse(old);
        let to = RoutePattern::parse(new);
        if let Some(missing) = to
            .params()
            .find(|param| !from.params().any(|p| p == *param))
        {
            return Err(ServerError::SegmentsMisdeclared {
                problems: format!("retired `{old}` does not capture `{missing}` for `{new}`"),
            });
        }
        let target = Arc::new(to);
        let route = Route::html(old, move |ctx: HttpContext<S>| {
            let target = Arc::clone(&target);
            async move {
                let params = ctx.params();
                let Some(path) = target.fill(|param| params.get(param)) else {
                    warn!(
                        route = ctx.route_template().unwrap_or_default(),
                        "a retired template's target has a capture the request lacks; answering 404"
                    );
                    return Err(AnyError::new(ServerError::NotFound {
                        method: Method::Get.to_string(),
                        path: PathShape::new(ctx.route_template().unwrap_or_default(), |_| true),
                    }));
                };
                let href = ctx
                    .address()
                    .map_or_else(|| path.clone(), |address| address.href(&path));
                let location = match ctx.query_raw() {
                    "" => href,
                    query => format!("{href}?{query}"),
                };
                match Redirect::moved_permanently(location) {
                    Ok(redirect) => {
                        debug!(
                            route = ctx.route_template().unwrap_or_default(),
                            "redirected a retired template"
                        );
                        ctx.redirect(redirect);
                        Ok(String::new())
                    }
                    Err(error) => {
                        warn!(
                            error = error.error_code(),
                            route = ctx.route_template().unwrap_or_default(),
                            "a retired template's target is not a usable redirect; answering 404"
                        );
                        Err(AnyError::new(ServerError::NotFound {
                            method: Method::Get.to_string(),
                            path: PathShape::new(ctx.route_template().unwrap_or_default(), |_| {
                                true
                            }),
                        }))
                    }
                }
            }
        })
        .skipping_segments();
        self.route(route)
    }

    /// Every word this router answers at as a literal, lower-cased: words a slug must never take.
    #[must_use]
    pub fn literal_segments(&self) -> std::collections::BTreeSet<String> {
        let first = |path: &str| {
            path.split('/')
                .find(|segment| !segment.is_empty())
                .map(str::to_owned)
        };
        let mut words: std::collections::BTreeSet<String> = self
            .routes
            .iter()
            .flat_map(|route| {
                route
                    .pattern()
                    .literals()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect();
        words.extend(self.documents.keys().filter_map(|path| first(path)));
        words.extend(self.assets.keys().filter_map(|path| first(path)));
        words.extend(self.disk_dirs.iter().filter_map(|(mount, _)| first(mount)));
        if let Some(space) = &self.addresses {
            words.extend(
                space
                    .prefixes()
                    .iter()
                    .map(|(prefix, _, _)| prefix.trim_matches('/').to_owned()),
            );
            words.extend(space.slugs().spellings().map(str::to_owned));
        }
        words
            .into_iter()
            .filter(|word| !word.is_empty())
            .map(|word| word.to_ascii_lowercase())
            .collect()
    }

    /// Every registered route, as `(method, template)` — what a conformance check walks.
    #[must_use]
    pub fn route_templates(&self) -> Vec<(Method, &str)> {
        self.routes
            .iter()
            .map(|route| (route.method(), route.pattern().raw()))
            .collect()
    }

    /// The path parameters a resolver is declared for — see [`segment`](Self::segment).
    #[must_use]
    pub fn declared_segments(&self) -> Vec<&str> {
        self.segments
            .iter()
            .map(|resolver| resolver.param())
            .collect()
    }

    /// The parameters `template` captures, in path order.
    #[must_use]
    pub fn template_params(template: &str) -> Vec<String> {
        RoutePattern::parse(template)
            .params()
            .map(str::to_owned)
            .collect()
    }

    /// What is wrong with the declared segments, one sentence each; empty when nothing is.
    #[must_use]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn segment_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !self.segments.is_empty() && self.authorizes_from_path {
            problems.push(
                "a router-level layer authorizes from a path parameter, before segments resolve"
                    .to_owned(),
            );
        }
        for resolver in &self.segments {
            let Some(scope) = resolver.scope() else {
                continue;
            };
            if !self
                .segments
                .iter()
                .any(|declared| declared.param() == scope)
            {
                problems.push(format!(
                    "`{}` is scoped by `{scope}`, which is not declared",
                    resolver.param()
                ));
            }
            for route in &self.routes {
                let params: Vec<&str> = route.pattern().params().collect();
                let Some(child) = params.iter().position(|p| *p == resolver.param()) else {
                    continue;
                };
                if !params[..child].contains(&scope) {
                    problems.push(format!(
                        "{} {} captures `{}` without `{scope}` before it",
                        route.method(),
                        route.pattern().raw(),
                        resolver.param()
                    ));
                }
            }
        }
        problems
    }

    /// The declared resolvers of `route`'s captures, in path order.
    fn stage_for<'a>(&'a self, route: &Route<S>) -> Option<SegmentStage<'a>> {
        SegmentStage::for_route(&self.segments, route)
    }

    /// The one `301` a page at a non-canonical spelling earns, once every record it names was affirmed.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn canonical_redirect(
        route: &Route<S>,
        ledger: &SegmentLedger,
        address: Option<&fundamental_core::address::RequestAddress>,
        query: &str,
    ) -> Option<ServerResponse> {
        let path = ledger.fill(route.pattern(), address.map(|address| address.locale()))?;
        let href = address.map_or_else(|| path.clone(), |address| address.href(&path));
        let location = match query {
            "" => href,
            query => format!("{href}?{query}"),
        };
        match Redirect::moved_permanently(location) {
            Ok(redirect) => {
                debug!(
                    route = route.pattern().raw(),
                    "redirected an entity to its canonical address"
                );
                // Private: the answer depends on who asked, so no shared cache may replay it.
                Some(
                    Self::redirected(&redirect)
                        .with_header("cache-control", "private, max-age=86400"),
                )
            }
            Err(error) => {
                warn!(
                    error = error.error_code(),
                    route = route.pattern().raw(),
                    "an entity's canonical address is not a usable redirect; serving in place"
                );
                None
            }
        }
    }

    /// Registers the page that answers an unmatched request from a browser.
    ///
    /// Never matched by path; it answers only a `404` whose `Accept` names `text/html`, and runs
    /// the full middleware chain. A not-found page must not call
    /// [`RequestAddress::alternates`](fundamental_core::address::RequestAddress::alternates).
    #[must_use]
    pub fn not_found(mut self, route: Route<S>) -> Self {
        self.not_found = Some(route);
        self
    }

    /// Registers the page that answers a person's navigation refused with `401` or `403`.
    #[must_use]
    pub fn gate_page(mut self, route: Route<S>) -> Self {
        self.gate = Some(route);
        self
    }

    /// Serves this router's routes at market-addressed URLs.
    ///
    /// A request is resolved before it is matched, so the route table does not grow with the
    /// locale set; the canonical redirect waits until the match says it is a page.
    #[must_use]
    pub fn with_addresses(mut self, addresses: Arc<AddressSpace>) -> Self {
        self.addresses = Some(addresses);
        self
    }

    /// Runs `middleware` around every route this router dispatches, outside each route's own chain.
    ///
    /// Router middleware runs outermost, in the order registered, whatever the order against
    /// [`route`](Self::route). It also runs around a [`not_found`](Self::not_found) page.
    #[must_use]
    pub fn middleware<M: Middleware<S>>(mut self, middleware: M) -> Self {
        // Read off the concrete type before it is erased.
        self.needs_principal |= <M as Middleware<S>>::NEEDS_PRINCIPAL;
        self.grants_authorization |= <M as Middleware<S>>::GRANTS_AUTHORIZATION;
        self.authorizes_from_path |= <M as Middleware<S>>::AUTHORIZES_FROM_PATH;
        self.reads_body |= <M as Middleware<S>>::READS_BODY;
        self.middleware.push(Box::new(middleware));
        self
    }

    /// The streamed routes with a body-reading layer in front of them, as `(method, path template)`.
    #[must_use]
    pub fn streamed_routes_reading_body(&self) -> Vec<(Method, &str)> {
        self.routes
            .iter()
            .filter(|route| matches!(route.body_policy(), BodyPolicy::Streamed(_)))
            .filter(|route| self.reads_body || route.reads_body())
            .map(|route| (route.method(), route.pattern().raw()))
            .collect()
    }

    /// How the backend must read `request`'s body, decided by the route that will serve it.
    #[must_use]
    pub fn body_policy(&self, request: &ServerRequest<S>) -> BodyPolicy {
        let path = request.path();
        let to_alias = self
            .addresses
            .as_ref()
            .zip(request.authority())
            .is_some_and(|(space, authority)| space.alias_target(authority).is_some());
        if to_alias {
            return BodyPolicy::Buffered;
        }
        let (_, found) = self.resolve(request, path);
        match found {
            Ok((route, _)) => route.body_policy().clone(),
            Err(_) => BodyPolicy::Buffered,
        }
    }

    /// The address `path` resolves to, and the route that serves it: the one lookup body policy and dispatch share.
    fn resolve<'a>(
        &'a self,
        request: &ServerRequest<S>,
        path: &str,
    ) -> (
        Option<ResolvedAddress>,
        ServerResult<(&'a Route<S>, PathParams)>,
    ) {
        let resolved = self
            .addresses
            .as_ref()
            .map(|space| space.resolve_on(request.authority(), path, request.locale()));
        let matched_path = resolved
            .as_ref()
            .map_or(path, ResolvedAddress::canonical_path);
        let found = self.find(request.method(), matched_path);
        (resolved, found)
    }

    /// The origin set the route serving `method` at `path` answers across origins, if any.
    #[must_use]
    pub fn cross_origin(&self, method: Method, path: &str) -> Option<&'static str> {
        self.find(method, path)
            .ok()
            .and_then(|(route, _)| route.cross_origin_set())
    }

    /// Every route that answers across origins, as `(method, path template, origin set)`.
    #[must_use]
    pub fn cross_origin_routes(&self) -> Vec<(Method, &str, &'static str)> {
        self.routes
            .iter()
            .filter_map(|route| {
                route
                    .cross_origin_set()
                    .map(|set| (route.method(), route.pattern().raw(), set))
            })
            .collect()
    }

    /// The routes that read no credential yet need a principal, as `(method, path template)`.
    #[must_use]
    pub fn cross_origin_routes_needing_a_principal(&self) -> Vec<(Method, &str)> {
        self.routes
            .iter()
            .filter(|route| {
                route.cross_origin_set().is_some()
                    && (self.needs_principal || route.needs_principal())
            })
            .map(|route| (route.method(), route.pattern().raw()))
            .collect()
    }

    /// The routes that can only ever be satisfied by an authenticated principal, as `(method, path template)`.
    #[must_use]
    pub fn principal_gated_routes(&self) -> Vec<(Method, &str)> {
        self.routes
            .iter()
            .filter(|route| self.needs_principal || route.needs_principal())
            .map(|route| (route.method(), route.pattern().raw()))
            .collect()
    }

    /// The writes on behalf of an identified caller that authorize no object, as `(method, path template)`.
    ///
    /// A route capturing a parameter whose guard resolves its object elsewhere fails too. A
    /// route marked [`Route::no_single_object`] is passed over; safe methods are not considered.
    #[must_use]
    pub fn authenticated_unauthorized_writes(&self) -> Vec<(Method, &str)> {
        self.routes
            .iter()
            .filter(|route| !route.method().is_safe())
            .filter(|route| self.needs_principal || route.needs_principal())
            .filter(|route| !route.declares_no_single_object())
            .filter(|route| {
                let authorized = self.grants_authorization || route.grants_authorization();
                let names_what_it_changes = !route.captures_anything()
                    || self.authorizes_from_path
                    || route.authorizes_from_path();
                !(authorized && names_what_it_changes)
            })
            .map(|route| (route.method(), route.pattern().raw()))
            .collect()
    }

    /// Serves the files under `dir` (recursively) at URLs rooted at `mount`, read into memory once.
    ///
    /// # Errors
    /// Returns [`ServerError::AssetLoad`] if `dir` exists but a directory or file
    /// under it cannot be read, and [`ServerError::PathTaken`] if a file under it
    /// would be served at a path a [`document`](Self::document) holds.
    pub fn static_dir(mut self, mount: &str, dir: impl AsRef<Path>) -> ServerResult<Self> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            // An absent dir would otherwise show up only as per-request 404s.
            warn!(mount, dir = %dir.display(), "static dir absent; serving no assets");
            return Ok(self);
        }
        let prefix = mount.trim_end_matches('/');
        let mut loaded = HashMap::new();
        load_assets(dir, dir, prefix, &mut loaded)?;
        if let Some(path) = self
            .documents
            .keys()
            .find(|path| loaded.contains_key(*path))
        {
            return Err(ServerError::PathTaken {
                path: path.clone(),
                held_by: "an asset",
            });
        }
        let addressed = self.hold_loaded(loaded);
        debug!(
            mount = prefix,
            count = self.assets.len(),
            addressed_references = addressed,
            "loaded static assets"
        );
        Ok(self)
    }

    /// Holds every loaded file, stylesheets last so every file they name is already versioned.
    fn hold_loaded(&mut self, loaded: HashMap<String, StaticAsset>) -> usize {
        let (stylesheets, others): (Vec<_>, Vec<_>) = loaded
            .into_iter()
            .partition(|(_, asset)| is_stylesheet(asset));
        for (key, asset) in others {
            self.hold_asset(key, asset);
        }
        let mut addressed = 0;
        for (key, asset) in stylesheets {
            let (asset, references) = self.addressed_stylesheet(&key, asset);
            addressed += references;
            self.hold_asset(key, asset);
        }
        addressed
    }

    /// Stamps `cache_control` on every static asset under `prefix` that carries no policy; longest prefix wins.
    #[must_use]
    pub fn static_cache_control(mut self, prefix: &str, cache_control: &'static str) -> Self {
        self.static_cache.push((prefix.to_owned(), cache_control));
        self
    }

    /// Serves the files under `dir` at `mount`, from memory or from disk.
    ///
    /// A disk mount's root is canonicalised once, and every request must resolve under it.
    ///
    /// # Errors
    /// Returns [`ServerError::AssetLoad`] if `dir` exists but cannot be read or
    /// canonicalised.
    pub fn static_dir_from(
        self,
        mount: &str,
        dir: impl AsRef<Path>,
        source: StaticSource,
    ) -> ServerResult<Self> {
        match source {
            StaticSource::Memory => self.static_dir(mount, dir),
            StaticSource::Disk => self.static_dir_on_disk(mount, dir.as_ref()),
        }
    }

    /// Tells every open page when the bundle it loads changes on disk, over `GET /_fundamental/live`.
    ///
    /// A router with no disk-served mount mounts nothing.
    ///
    /// # Errors
    /// [`ServerError::RouteConflict`] if a route already holds `GET /_fundamental/live`.
    pub fn live_reload(self) -> ServerResult<Self> {
        if self.disk_dirs.is_empty() {
            warn!(
                "live reload asked for with no disk-served mount; nothing can change, so none is mounted"
            );
            return Ok(self);
        }
        let live = LiveReload::new(
            self.disk_dirs
                .iter()
                .map(|(_, root)| root.clone())
                .collect(),
        );
        let ended = live.clone();
        let mut router = self
            .route(Route::streaming_response(
                Method::Get,
                LIVE_PATH,
                move |_: HttpContext<S>| {
                    let stream = live.stream();
                    async move {
                        Ok(StreamedResponse::with_unknown_length(
                            "text/event-stream",
                            stream,
                        ))
                    }
                },
            ))?
            .static_asset(
                LIVE_SCRIPT_PATH,
                StaticAsset::typed(
                    LIVE_SCRIPT.as_bytes().to_vec(),
                    "text/javascript; charset=utf-8",
                ),
            );
        debug!(path = LIVE_PATH, "live reload mounted");
        router.extensions = router.extensions.with(LiveReloadMounted(ended));
        Ok(router)
    }

    /// [`live_reload`](Self::live_reload) when `on`; otherwise the router unchanged.
    ///
    /// # Errors
    /// As [`live_reload`](Self::live_reload).
    pub fn live_reload_when(self, on: bool) -> ServerResult<Self> {
        if on { self.live_reload() } else { Ok(self) }
    }

    /// Whether [`live_reload`](Self::live_reload) is mounted.
    #[must_use]
    pub fn serves_live_reload(&self) -> bool {
        self.extensions.get::<LiveReloadMounted>().is_some()
    }

    /// Ends every stream the router holds open, so a graceful stop is not held by an open page.
    pub fn end_open_streams(&self) {
        if let Some(LiveReloadMounted(live)) = self.extensions.get::<LiveReloadMounted>() {
            live.end_open_streams();
        }
    }

    /// Register a disk-served mount: canonicalise the root and remember it.
    fn static_dir_on_disk(mut self, mount: &str, dir: &Path) -> ServerResult<Self> {
        if !dir.is_dir() {
            warn!(mount, dir = %dir.display(), "static dir absent; serving no assets");
            return Ok(self);
        }
        let root = fs::canonicalize(dir).map_err(|e| ServerError::AssetLoad {
            path: dir.display().to_string(),
            message: format!("could not canonicalize the static dir: {e}"),
        })?;
        let prefix = mount.trim_end_matches('/').to_string();
        if let Some(path) = self
            .documents
            .keys()
            .find(|path| path.starts_with(prefix.as_str()))
        {
            return Err(ServerError::PathTaken {
                path: path.clone(),
                held_by: "a disk-served mount",
            });
        }
        warn!(
            mount = %prefix,
            dir = %root.display(),
            "serving static assets from disk (development); each request reads the file"
        );
        self.disk_dirs.push((prefix, root));
        Ok(self)
    }

    /// Serves `asset` from memory at the exact `path`; a [`document`](Self::document) there still wins.
    #[must_use]
    pub fn static_asset(mut self, path: &str, asset: StaticAsset) -> Self {
        if self.documents.contains_key(path) {
            warn!(
                path,
                "a static asset was registered at a document's path; the document is served"
            );
        }
        self.hold_asset(path.to_owned(), asset);
        self
    }

    /// Answers addressed requests for `path` (or every file under a `/`) before admission.
    ///
    /// # Errors
    /// [`ServerError::NothingToAnswerEarly`] (`API_032`) when no asset or every-origin
    /// document is served at `path`, or under it.
    pub fn before_admission(mut self, path: &str) -> ServerResult<Self> {
        let declared = |key: &str| match path.ends_with('/') {
            true => key.starts_with(path),
            false => key == path,
        };
        let serves = self.assets.keys().any(|key| declared(key))
            || self.documents.iter().any(|(key, scoped)| {
                declared(key) && scoped.iter().any(|(origin, _)| origin.is_none())
            });
        if !serves {
            return Err(ServerError::NothingToAnswerEarly {
                path: path.to_owned(),
            });
        }
        debug!(path, "answering addressed requests before admission");
        self.before_admission.push(path.to_owned());
        Ok(self)
    }

    /// The version every asset and every-origin document is linked by, by path.
    #[must_use]
    pub fn asset_addresses(&self) -> Arc<AssetAddresses> {
        Arc::clone(&self.asset_addresses)
    }

    /// Holds `asset` at `key`, linked by its version unless a document is served there.
    fn hold_asset(&mut self, key: String, asset: StaticAsset) {
        if !self.documents.contains_key(&key) {
            let addresses = Arc::make_mut(&mut self.asset_addresses);
            match asset.version() {
                Some(version) => addresses.insert(&key, version.clone()),
                None => addresses.remove(&key),
            }
        }
        self.assets.insert(key, asset);
    }

    /// `asset`, a stylesheet, with every `url(…)` naming a held file rewritten to its address, and the count.
    fn addressed_stylesheet(&self, key: &str, asset: StaticAsset) -> (StaticAsset, usize) {
        let Ok(css) = std::str::from_utf8(asset.bytes()) else {
            warn!(
                path = key,
                "a stylesheet is not UTF-8; its references are left unversioned"
            );
            return (asset, 0);
        };
        let (rewritten, references) =
            address_stylesheet(key, css, |target| self.asset_addresses.version(target));
        match references {
            0 => (asset, 0),
            _ => (asset.rewritten(rewritten.into_bytes()), references),
        }
    }

    /// Publishes `asset` at the exact `path` as a document, before any asset, disk mount or route.
    ///
    /// # Errors
    /// [`ServerError::PathTaken`] (`API_019`) when another document, an asset, a disk
    /// mount or a `GET` route already serves `path`.
    pub fn document(self, path: &str, asset: StaticAsset) -> ServerResult<Self> {
        self.scoped_document(None, path, asset)
    }

    /// Publishes `asset` at `path` for requests addressed to `origin` only.
    ///
    /// # Errors
    /// [`ServerError::PathTaken`] (`API_019`) as for [`document`](Self::document), or when
    /// a document for the same origin already holds `path`.
    pub fn document_on(
        self,
        origin: WebOrigin,
        path: &str,
        asset: StaticAsset,
    ) -> ServerResult<Self> {
        self.scoped_document(Some(origin), path, asset)
    }

    fn scoped_document(
        mut self,
        origin: Option<WebOrigin>,
        path: &str,
        asset: StaticAsset,
    ) -> ServerResult<Self> {
        let same_scope = self
            .documents
            .get(path)
            .is_some_and(|scoped| scoped.iter().any(|(known, _)| *known == origin));
        let held_by = if same_scope {
            Some("another document")
        } else if self.assets.contains_key(path) {
            Some("an asset")
        } else if self
            .disk_dirs
            .iter()
            .any(|(prefix, _)| path.starts_with(prefix.as_str()))
        {
            Some("a disk-served mount")
        } else if self
            .routes
            .iter()
            .any(|route| route.method() == Method::Get && route.pattern().raw() == path)
        {
            Some("a route")
        } else {
            None
        };
        if let Some(held_by) = held_by {
            return Err(ServerError::PathTaken {
                path: path.to_owned(),
                held_by,
            });
        }
        debug!(
            path,
            origin = origin.as_ref().map_or("every origin", WebOrigin::as_str),
            content_type = asset.content_type(),
            "published a document"
        );
        // One origin's document is never linked: the link would be wrong on every other origin.
        if origin.is_none()
            && let Some(version) = asset.version()
        {
            Arc::make_mut(&mut self.asset_addresses).insert(path, version.clone());
        }
        self.documents
            .entry(path.to_owned())
            .or_default()
            .push((origin, asset));
        Ok(self)
    }

    /// The document at `path` for `authority`: that origin's, otherwise the every-origin one.
    fn document_for(&self, path: &str, authority: Option<&str>) -> Option<&StaticAsset> {
        let scoped = self.documents.get(path)?;
        authority
            .and_then(|authority| {
                scoped.iter().find(|(origin, _)| {
                    origin
                        .as_ref()
                        .is_some_and(|origin| origin.serves_authority(authority))
                })
            })
            .or_else(|| scoped.iter().find(|(origin, _)| origin.is_none()))
            .map(|(_, asset)| asset)
    }

    /// Whether a `GET` route answering `text/html` serves `path`, a canonical path.
    #[must_use]
    pub fn serves_page(&self, path: &str) -> bool {
        self.routes.iter().any(|route| {
            route.method() == Method::Get
                && route.content_type().starts_with("text/html")
                && route.pattern().matches(path).is_some()
        })
    }

    /// Hands `value` to every handler, read back by type through [`HttpContext::extension`].
    #[must_use]
    pub fn with_extension<T: std::any::Any + Send + Sync>(mut self, value: T) -> Self {
        self.extensions = self.extensions.with(value);
        self
    }

    /// Every page this router serves at a fixed path, sorted.
    #[must_use]
    pub fn page_paths(&self) -> Vec<&str> {
        let mut paths: Vec<&str> = self
            .routes
            .iter()
            .filter(|route| {
                route.method() == Method::Get && route.content_type().starts_with("text/html")
            })
            .map(|route| route.pattern().raw())
            .filter(|template| !template.contains('{'))
            .collect();
        paths.sort_unstable();
        paths.dedup();
        paths
    }

    /// The address space this router resolves requests against, if one is attached.
    #[must_use]
    pub fn addresses(&self) -> Option<&Arc<AddressSpace>> {
        self.addresses.as_ref()
    }

    /// Reads `path` out of the first disk-served mount that claims it; `None` for every rejection.
    fn read_from_disk(&self, path: &str) -> Option<(String, StaticAsset)> {
        for (prefix, root) in &self.disk_dirs {
            let Some(file) = resolve_on_disk(prefix, root, path) else {
                continue;
            };
            let key = canonical_asset_key(prefix, root, &file);
            match fs::read(&file) {
                Ok(bytes) => return Some((key, StaticAsset::new(&file, bytes))),
                // The path resolved and the read failed: worth a line, never a 500.
                Err(e) => {
                    warn!(path = %key, error = %e, "could not read a static file");
                    return None;
                }
            }
        }
        None
    }

    /// The answer for the static asset or document at `key`: `immutable` at its exact version, `304` on a match.
    fn static_answer(
        &self,
        key: &str,
        asset: &StaticAsset,
        request: StaticRequest<'_>,
    ) -> ServerResponse {
        let addressed = request.from_memory && Self::names_version(asset, request.query);
        let cache_control = if addressed {
            Some(IMMUTABLE)
        } else if request.query.split('&').any(|pair| pair.starts_with("v=")) {
            Some(REVALIDATE)
        } else {
            asset.cache_control().or_else(|| {
                self.static_cache
                    .iter()
                    .filter(|(prefix, _)| key.starts_with(prefix.as_str()))
                    .max_by_key(|(prefix, _)| prefix.len())
                    .map(|(_, cache_control)| *cache_control)
            })
        };
        let etag = asset.version().map(|version| format!("\"{version}\""));
        let unchanged = etag
            .as_deref()
            .zip(request.if_none_match)
            .is_some_and(|(etag, header)| if_none_match(header, etag));
        let response = match unchanged {
            true => ServerResponse::new(304, Vec::new())
                .with_language_invariant()
                .with_preference_invariant(),
            false => asset_response(asset),
        };
        let response = match etag {
            Some(etag) => response.with_header("etag", etag),
            None => response,
        };
        let response = match cache_control {
            Some(cache_control) => response.with_header("cache-control", cache_control),
            None => response,
        };
        debug!(path = key, addressed, unchanged, "answered a static file");
        response
    }

    /// Whether `query` is exactly `v=` and the version of `asset`'s bytes.
    fn names_version(asset: &StaticAsset, query: &str) -> bool {
        asset
            .version()
            .is_some_and(|version| query.strip_prefix("v=") == Some(version.as_str()))
    }

    /// The answer to a `GET` the backend gives before admission, or `None` to admit and dispatch it.
    #[must_use]
    pub fn addressed_answer(
        &self,
        path: &str,
        query: &str,
        authority: Option<&str>,
        if_none_match: Option<&str>,
    ) -> Option<ServerResponse> {
        let declared = self
            .before_admission
            .iter()
            .any(|declared| match declared.ends_with('/') {
                true => path.starts_with(declared.as_str()),
                false => path == declared,
            });
        if !declared {
            return None;
        }
        if let (Some(space), Some(authority)) = (self.addresses.as_ref(), authority)
            && space.alias_target(authority).is_some()
        {
            return None;
        }
        let asset = self
            .document_for(path, authority)
            .or_else(|| self.assets.get(path))?;
        if !Self::names_version(asset, query) {
            return None;
        }
        Some(self.static_answer(
            path,
            asset,
            StaticRequest {
                query,
                if_none_match,
                from_memory: true,
            },
        ))
    }

    /// `path` as it may be recorded: the segments this router declared, and a class for every other.
    fn shape(&self, path: &str) -> PathShape {
        PathShape::new(path, |segment| self.declares(segment))
    }

    /// Whether `segment` is one this router wrote itself, and so cannot be a caller's value.
    fn declares(&self, segment: &str) -> bool {
        let in_path = |path: &str| path.split('/').any(|part| part == segment);
        self.routes
            .iter()
            .map(Arc::as_ref)
            .chain(self.not_found.iter())
            .any(|route| route.pattern().has_literal(segment))
            || self.disk_dirs.iter().any(|(prefix, _)| in_path(prefix))
            || self.assets.keys().any(|key| in_path(key))
            || self.addresses.as_ref().is_some_and(|space| {
                space
                    .prefixes()
                    .iter()
                    .any(|(prefix, _, _)| in_path(prefix))
            })
    }

    /// The number of registered routes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// Whether no routes are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// Dispatches `request` to the route that serves it and returns its response; it never fails.
    ///
    /// The span is built by hand rather than with `#[instrument]`, because a span's parent is
    /// fixed when it is entered and the caller's trace context must be met first. Every field is
    /// a value this server produced.
    pub async fn dispatch(&self, mut request: ServerRequest<S>) -> ServerResponse {
        let request_id = request.resolve_request_id();
        let span = tracing::info_span!(
            "server.dispatch",
            method = %request.method(),
            request_id = %request_id.id(),
            // Exactly one of `route` and `path` is recorded: `path` only when no template matched,
            // and then as a shape, since a concrete path can embed a joinable identifier.
            route = tracing::field::Empty,
            path = tracing::field::Empty,
            status = tracing::field::Empty,
        );
        // A backend that met the trace context at its own edge attaches no policy, and this span inherits.
        if let Some(policy) = request.inbound_trace_context() {
            policy.meet(&span, TraceParent::from_carrier(request.headers()));
        }

        async move {
            request_id.log();
            let page_request = self.error_page_request(&request);
            let response = self.dispatch_within(request).await;
            self.answer_navigation(response, page_request).await
        }
        .instrument(span)
        .await
    }

    /// The request the [`error_page`](Self::error_page) would render with, taken before dispatch consumes it.
    #[must_use]
    pub fn error_page_request(&self, request: &ServerRequest<S>) -> Option<ServerRequest<S>> {
        let page = self.error_page.as_ref()?;
        match Audience::classify(Some(request.method()), request.headers()) {
            Audience::Navigation => Some((page.request_for)(request)),
            Audience::Programmatic => None,
        }
    }

    /// Answers a navigation's error with the [`error_page`](Self::error_page), and every other response as it is.
    ///
    /// The page keeps the error's status and headers, runs none of the router's middleware,
    /// and is never cached or indexed; a `404` gets the [`not_found`](Self::not_found) page.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub async fn answer_navigation(
        &self,
        response: ServerResponse,
        page_request: Option<ServerRequest<S>>,
    ) -> ServerResponse {
        let (Some(shown), Some(page)) = (response.shown_error(), self.error_page.as_ref()) else {
            return response;
        };
        let response = response.with_header("vary", "Sec-Fetch-Mode, Accept");
        // A program's request keeps the envelope; no later call need ask again.
        let Some(request) = page_request else {
            return response.unmarked();
        };
        let request = request.with_shown_error(shown);
        let resolved = self
            .addresses
            .as_ref()
            .map(|space| space.resolve_on(request.authority(), request.path(), request.locale()));
        let (request, from_path) = self.adopt_address(request, resolved.as_ref(), false);
        // A route's `404` gets the not-found page; a refusal gets the gate.
        let route = match (shown.status(), &self.not_found, &self.gate) {
            (404, Some(not_found), _) => not_found,
            (401 | 403, _, Some(gate)) => gate,
            _ => &page.route,
        };
        let rendered = self
            .run(route, PathParams::default(), request, shown.status(), &[])
            .await;
        if let Some(failure) = rendered.shown_error() {
            warn!(
                error = shown.code(),
                status = shown.status(),
                page_error = failure.code(),
                "the error page could not be rendered; answered the navigation with the envelope"
            );
            return response.unmarked();
        }
        debug!(
            error = shown.code(),
            status = shown.status(),
            "answered a navigation with the error page"
        );
        let answered = response
            .answered_with(rendered)
            .with_replaced_header("cache-control", "no-store")
            .with_header("x-robots-tag", "noindex");
        match from_path {
            true => answered.with_language_invariant(),
            false => answered,
        }
    }

    /// The dispatch body, run inside the request span its caller built.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "readability baseline"
    )]
    async fn dispatch_within(&self, request: ServerRequest<S>) -> ServerResponse {
        let method = request.method();
        let path = request.path().to_owned();

        // A static asset takes precedence over a route, for a `GET` only.
        if let Some(redirect) = self.alias_redirect(&request, &path) {
            tracing::Span::current().record("status", redirect.status());
            return redirect;
        }
        let static_request = StaticRequest {
            query: request.query(),
            if_none_match: request.headers().get("if-none-match"),
            from_memory: true,
        };
        if method == Method::Get
            && let Some(document) = self.document_for(&path, request.authority())
        {
            debug!(path = %path, "served document");
            let response = self.static_answer(&path, document, static_request);
            tracing::Span::current().record("path", path.as_str());
            tracing::Span::current().record("status", response.status());
            return response;
        }
        if method == Method::Get
            && let Some(asset) = self.assets.get(&path)
        {
            debug!(path = %path, "served static asset");
            let response = self.static_answer(&path, asset, static_request);
            tracing::Span::current().record("path", path.as_str());
            tracing::Span::current().record("status", response.status());
            return response;
        }
        if method == Method::Get
            && let Some((key, asset)) = self.read_from_disk(&path)
        {
            // The request path may have reached the file through `./` and `//` padding.
            debug!(path = %key, "served static asset from disk");
            let response = self.static_answer(
                &key,
                &asset,
                StaticRequest {
                    from_memory: false,
                    ..static_request
                },
            );
            tracing::Span::current().record("path", key.as_str());
            tracing::Span::current().record("status", response.status());
            return response;
        }

        // After static assets, before matching: the route table holds canonical paths.
        let (resolved, found) = self.resolve(&request, &path);
        let matched_path = resolved
            .as_ref()
            .map_or(path.as_str(), ResolvedAddress::canonical_path);

        // A document is also found at the spelling the address space printed for it.
        if method == Method::Get
            && matched_path != path
            && let Some(document) = self.document_for(matched_path, request.authority())
        {
            debug!(path = %matched_path, "served document at a spelling of its path");
            let response = asset_response(document);
            tracing::Span::current().record("path", matched_path);
            tracing::Span::current().record("status", response.status());
            return response;
        }

        let response = match found {
            Ok((route, params)) => {
                tracing::Span::current().record("route", route.pattern().raw());
                match self.off_address(resolved.as_ref(), route, method, request.query()) {
                    Some(redirect) => redirect,
                    None => {
                        let (request, from_path) =
                            self.adopt_address(request, resolved.as_ref(), true);
                        let response = self
                            .run(route, params, request, 200, &self.middleware)
                            .await;
                        match from_path {
                            true => response.with_language_invariant(),
                            false => response,
                        }
                    }
                }
            }
            Err(routing_error) => {
                // The path is the caller's, so only its shape is recorded.
                let shape = self.shape(&path);
                tracing::Span::current().record("path", shape.as_str());
                if method != Method::Get || path != CHROME_DEVTOOLS_WORKSPACE_PATH {
                    warn!(
                        error = routing_error.error_code(),
                        "request matched no route"
                    );
                }
                match self.not_found_page(&routing_error, &request) {
                    Some(page) => {
                        debug!(path = %shape, "answered with the not-found page");
                        // A reader who mistypes a URL inside a market is still in that market.
                        let (request, from_path) =
                            self.adopt_address(request, resolved.as_ref(), false);
                        let response = self
                            .run(page, PathParams::default(), request, 404, &self.middleware)
                            .await;
                        match from_path {
                            true => response.with_language_invariant(),
                            false => response,
                        }
                    }
                    None => self.routing_refusal(&routing_error, matched_path),
                }
            }
        };

        tracing::Span::current().record("status", response.status());
        response
    }

    /// The permanent redirect for a request addressed to an alias origin: `301` for a `GET`, `308` otherwise.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn alias_redirect(&self, request: &ServerRequest<S>, path: &str) -> Option<ServerResponse> {
        let target = self
            .addresses
            .as_ref()?
            .alias_target(request.authority()?)?;
        let query = request.query();
        let location = match query.is_empty() {
            true => format!("{target}{path}"),
            false => format!("{target}{path}?{query}"),
        };
        let redirect = match request.method() {
            Method::Get => Redirect::moved_permanently(location),
            _ => Redirect::permanent(location),
        };
        match redirect {
            Ok(redirect) => {
                debug!(
                    target = target.as_str(),
                    status = redirect.status(),
                    "redirected an alias to its origin"
                );
                Some(Self::redirected(&redirect))
            }
            Err(error) => {
                warn!(
                    error = error.error_code(),
                    target = target.as_str(),
                    path_bytes = path.len(),
                    "an alias redirect target is not usable; answering on the alias"
                );
                None
            }
        }
    }

    /// Attaches the resolved address to `request`, and answers whether the locale came from the path.
    ///
    /// Both dispatch outcomes call it; `served` is `false` for the not-found page.
    fn adopt_address(
        &self,
        request: ServerRequest<S>,
        resolved: Option<&ResolvedAddress>,
        served: bool,
    ) -> (ServerRequest<S>, bool) {
        match (resolved, &self.addresses) {
            (Some(resolved), Some(space)) => (
                request.addressed(resolved, Arc::clone(space), served),
                resolved.locale_from_path(),
            ),
            _ => (request, false),
        }
    }

    /// The `301` for a `GET` of a `text/html` page not at its canonical address, the query carried over.
    ///
    /// Decided after the match, because a bundle or `robots.txt` must be served where it was asked.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn off_address(
        &self,
        resolved: Option<&ResolvedAddress>,
        route: &Route<S>,
        method: Method,
        query: &str,
    ) -> Option<ServerResponse> {
        let target = resolved?.redirect_target()?;
        if method != Method::Get || !route.content_type().starts_with("text/html") {
            return None;
        }
        // A browser reads `/\host` as `//host`, another origin: serve the page in place.
        if target.starts_with("//") || target.starts_with("/\\") {
            warn!(
                route = route.pattern().raw(),
                "canonical address would read as another origin; serving in place"
            );
            return None;
        }
        let target: Cow<'_, str> = match query.is_empty() || target.contains('?') {
            true => Cow::Borrowed(target),
            false => Cow::Owned(format!("{target}?{query}")),
        };
        let target = target.as_ref();
        match Redirect::moved_permanently(target) {
            Ok(redirect) => {
                // The route, never the target: the target carries the caller's query.
                debug!(
                    route = route.pattern().raw(),
                    query_bytes = query.len(),
                    "redirected to the canonical address"
                );
                Some(Self::redirected(&redirect))
            }
            Err(error) => {
                // The query is the caller's; serve in place, and log the route and the query's length only.
                warn!(
                    error = error.error_code(),
                    route = route.pattern().raw(),
                    query_bytes = query.len(),
                    "canonical address is not a usable redirect target; serving in place"
                );
                None
            }
        }
    }

    /// The not-found page, for a `404` whose caller asked for HTML.
    fn not_found_page(
        &self,
        routing_error: &ServerError,
        request: &ServerRequest<S>,
    ) -> Option<&Route<S>> {
        if !matches!(routing_error, ServerError::NotFound { .. }) {
            return None;
        }
        if !accepts_html(request.headers().get("accept")) {
            return None;
        }
        self.not_found.as_ref()
    }

    /// Runs `route`'s middleware chain and handler after building the request's context.
    ///
    /// `default_status` is `200` for a matched route, `404` for the not-found page.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::too_many_arguments,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    async fn run(
        &self,
        route: &Route<S>,
        params: PathParams,
        request: ServerRequest<S>,
        default_status: u16,
        outer: &[Box<dyn ErasedMiddleware<S>>],
    ) -> ServerResponse {
        let method = request.method();
        let parts = request.into_parts();
        let body = parts.body;
        let body_stream = parts.body_stream;

        let context = HttpContext::new(parts.state, parts.request_id)
            .with_params(params)
            .with_principal(parts.principal)
            .with_locale(parts.locale)
            .with_preferences(parts.preferences)
            .with_address(parts.address)
            .with_asset_addresses(Arc::clone(&self.asset_addresses))
            .with_extensions(self.extensions.clone())
            .with_peer(parts.peer)
            .with_query(parts.query)
            .with_headers(parts.headers)
            .with_route_template(route.pattern().template())
            .with_method(method)
            .with_body_stream(body_stream)
            .with_shown_error(parts.shown_error);
        let action_routes: Arc<dyn std::any::Any + Send + Sync> = self.action_routes.clone();
        let context = context.with_action_routes(action_routes);

        // The context moves into the handler, so keep its sinks to drain afterwards. A recorded
        // redirect is honoured on the error arm too, with the error still logged.
        let response_headers = context.response_headers();
        let response_status = context.response_status();
        let response_redirect = context.response_redirect();
        let response_stream = context.response_stream_slot();
        let ledger = context.segments().clone();
        let authorized = context.authorized_objects().clone();
        let address = context.address().cloned();
        let query = context.query_raw().to_owned();
        let stage = self.stage_for(route);
        let page = method == Method::Get && route.content_type().starts_with("text/html");

        let response = match route.call_staged(outer, stage, context, body).await {
            Ok(body) => match response_redirect.take() {
                Some(redirect) => Self::redirected(&redirect),
                None => {
                    let status = response_status.take().unwrap_or(default_status);
                    let canonical = match page && (200..300).contains(&status) && ledger.moved() {
                        true if ledger.disclosable(&authorized) => {
                            Self::canonical_redirect(route, &ledger, address.as_ref(), &query)
                        }
                        true => {
                            debug!(
                                route = route.pattern().raw(),
                                "a non-canonical segment was not affirmed; served in place"
                            );
                            None
                        }
                        false => None,
                    };
                    match (canonical, response_stream.take()) {
                        (Some(redirect), _) => redirect,
                        (None, Some(stream)) => ServerResponse::streamed(status, stream),
                        // `204 No Content` carries no content: no body, no type (RFC 9110 §15.3.5).
                        (None, None) if status == 204 => {
                            ServerResponse::new(status, Vec::new()).with_content_type("")
                        }
                        (None, None) => ServerResponse::new(status, body)
                            .with_content_type(route.content_type()),
                    }
                }
            },
            Err(error) => {
                let inner = error.get();
                match response_redirect.take() {
                    Some(redirect) => {
                        warn!(
                            error = inner.error_code(),
                            status = inner.http_status_code(),
                            redirect = redirect.status(),
                            "handler returned an error and redirected instead of answering it"
                        );
                        Self::redirected(&redirect)
                    }
                    None => {
                        warn!(
                            error = inner.error_code(),
                            status = inner.http_status_code(),
                            "handler returned an error"
                        );
                        ServerResponse::from_error(inner)
                    }
                }
            }
        };

        // Recorded headers ride the error too, so a challenge rides its 4xx. A recorded
        // `Cache-Control` replaces the router's; a `Content-Type` never relabels an error body.
        response_headers
            .take()
            .into_iter()
            .fold(response, |response, (name, value)| {
                if name.eq_ignore_ascii_case("cache-control") {
                    response.with_replaced_header(name, value)
                } else if name.eq_ignore_ascii_case("content-type") {
                    if (200..300).contains(&response.status()) {
                        response.with_content_type(value)
                    } else {
                        response
                    }
                } else {
                    response.with_header(name, value)
                }
            })
    }

    /// Renders a recorded [`Redirect`] as the reply, with one `Location`.
    fn redirected(redirect: &Redirect) -> ServerResponse {
        ServerResponse::new(redirect.status(), Vec::new())
            .with_header("location", redirect.location())
    }

    /// Renders a routing refusal, stamping `Allow` on a `405` (RFC 9110 §15.5.6).
    fn routing_refusal(&self, error: &ServerError, path: &str) -> ServerResponse {
        let response = ServerResponse::from_error(error);
        match error {
            ServerError::MethodNotAllowed { .. } => {
                response.with_header("Allow", self.allowed_methods(path))
            }
            _ => response,
        }
    }

    /// The methods this router serves at `path`, as an `Allow` value; `HEAD` with `GET`, never `OPTIONS`.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn allowed_methods(&self, path: &str) -> String {
        let mut allowed: Vec<&'static str> = Vec::new();
        for route in &self.routes {
            if route.pattern().matches(path).is_some() {
                let token = route.method().as_str();
                if !allowed.contains(&token) {
                    allowed.push(token);
                }
                if route.method() == Method::Get && !allowed.contains(&"HEAD") {
                    allowed.push("HEAD");
                }
            }
        }
        allowed.join(", ")
    }

    /// Finds the route serving `method` at `path`, telling `404` from `405`.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn find(&self, method: Method, path: &str) -> ServerResult<(&Route<S>, PathParams)> {
        let mut path_matched = false;
        for route in &self.routes {
            if let Some(params) = route.pattern().matches(path) {
                path_matched = true;
                if route.method() == method {
                    return Ok((route, params));
                }
            }
        }

        if path_matched {
            Err(ServerError::MethodNotAllowed {
                method: method.to_string(),
                path: self.shape(path),
            })
        } else {
            Err(ServerError::NotFound {
                method: method.to_string(),
                path: self.shape(path),
            })
        }
    }
}

/// The policy of an address that names the exact bytes it serves (RFC 8246).
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// The policy of an address naming a version this server does not hold.
const REVALIDATE: &str = "no-cache";

/// What a static answer reads of its request.
#[derive(Clone, Copy)]
struct StaticRequest<'a> {
    query: &'a str,
    if_none_match: Option<&'a str>,
    /// Whether the bytes come from memory; a disk read can change under its version.
    from_memory: bool,
}

/// Whether `asset` is a stylesheet, whose `url(…)`s are addressed when it is loaded.
fn is_stylesheet(asset: &StaticAsset) -> bool {
    asset.content_type().starts_with("text/css")
}

/// The response for an asset or document; its cache policy is [`Router::static_answer`]'s.
fn asset_response(asset: &StaticAsset) -> ServerResponse {
    // Byte-identical whoever asks, so no language `Vary` to split the CDN cache.
    ServerResponse::new(200, asset.bytes().to_vec())
        .with_content_type(asset.content_type())
        .with_language_invariant()
        // An asset is also preference-invariant; an addressed page is not.
        .with_preference_invariant()
}

/// Whether an `Accept` header asks for HTML; `*/*` does, an absent header does not.
fn accepts_html(accept: Option<&str>) -> bool {
    let Some(accept) = accept else {
        return false;
    };
    accept.split(',').any(|entry| {
        let media_type = entry.split(';').next().unwrap_or("").trim();
        media_type.eq_ignore_ascii_case("text/html")
            || media_type == "*/*"
            || media_type.eq_ignore_ascii_case("text/*")
    })
}

/// The key a disk-served file is recorded under: its mount prefix and its path below the root.
fn canonical_asset_key(prefix: &str, root: &Path, file: &Path) -> String {
    let relative = file.strip_prefix(root).unwrap_or(file);
    let mut key = String::from(prefix);
    for component in relative.components() {
        key.push('/');
        key.push_str(&component.as_os_str().to_string_lossy());
    }
    key
}

/// Reads a request path out of a disk-served mount, or `None`; cheap structural refusals first.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn resolve_on_disk(prefix: &str, root: &Path, path: &str) -> Option<PathBuf> {
    let relative = path.strip_prefix(prefix)?.trim_start_matches('/');
    if relative.is_empty() {
        // A directory: no listing.
        return None;
    }

    // Refuse `..` and root components before touching the filesystem; the prefix check below backs it.
    let candidate = Path::new(relative);
    if candidate.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        warn!(
            mount = prefix,
            "refused a static path with a traversal component"
        );
        return None;
    }

    let joined = root.join(candidate);
    // Resolves symlinks too, so a link inside the mount cannot point out of it.
    let resolved = fs::canonicalize(&joined).ok()?;
    if !resolved.starts_with(root) {
        warn!(
            mount = prefix,
            resolved = %resolved.display(),
            "refused a static path outside its mount"
        );
        return None;
    }
    if !resolved.is_file() {
        return None;
    }
    Some(resolved)
}

/// Recursively reads `dir` into `out`, keyed by `prefix` plus the path relative to `root`.
fn load_assets(
    root: &Path,
    dir: &Path,
    prefix: &str,
    out: &mut HashMap<String, StaticAsset>,
) -> ServerResult<()> {
    let asset_error = |path: &Path, error: &std::io::Error| ServerError::AssetLoad {
        path: path.display().to_string(),
        message: error.to_string(),
    };

    for entry in fs::read_dir(dir).map_err(|e| asset_error(dir, &e))? {
        let entry = entry.map_err(|e| asset_error(dir, &e))?;
        let path = entry.path();
        if path.is_dir() {
            load_assets(root, &path, prefix, out)?;
            continue;
        }
        let bytes = fs::read(&path).map_err(|e| asset_error(&path, &e))?;
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let url = format!("{prefix}/{}", relative.to_string_lossy().replace('\\', "/"));
        out.insert(url, StaticAsset::new(&path, bytes));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InboundTraceContext;
    use fundamental_auth::{AuthenticatedPrincipal, UserId};
    use serde::{Deserialize, Serialize};

    #[derive(Clone)]
    struct State {
        greeting: &'static str,
    }

    #[derive(Deserialize)]
    struct Name {
        name: String,
    }

    #[derive(Serialize, Deserialize)]
    struct Greeting {
        text: String,
    }

    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn router() -> Router<State> {
        Router::new()
            .route(Route::post(
                "/greet",
                |ctx: HttpContext<State>, input: Name| async move {
                    Ok(Greeting {
                        text: format!("{}, {}", ctx.state().greeting, input.name),
                    })
                },
            ))
            .expect("unique route")
            .route(Route::get(
                "/users/{id}",
                |ctx: HttpContext<State>, _: ()| async move {
                    let id: u64 = ctx.param("id")?;
                    Ok(Greeting {
                        text: format!("user {id}"),
                    })
                },
            ))
            .expect("unique route")
            .route(
                Route::get("/me", |_ctx: HttpContext<State>, _: ()| async move {
                    Ok(Greeting {
                        text: "you".to_owned(),
                    })
                })
                .middleware(crate::auth),
            )
            .expect("unique route")
            .route(Route::get(
                "/nonce",
                |ctx: HttpContext<State>, _: ()| async move {
                    ctx.set_response_header("DPoP-Nonce", "n-ok");
                    Ok(Greeting {
                        text: "ok".to_owned(),
                    })
                },
            ))
            .expect("unique route")
            .route(Route::get(
                "/file",
                |ctx: HttpContext<State>, _: ()| async move {
                    ctx.set_response_header("Content-Type", "application/pdf");
                    Ok(Greeting {
                        text: "pdf".to_owned(),
                    })
                },
            ))
            .expect("unique route")
            .route(Route::get(
                "/file-refused",
                |ctx: HttpContext<State>, _: ()| async move {
                    ctx.set_response_header("Content-Type", "application/pdf");
                    let missing: u64 = ctx.param("id")?;
                    Ok(Greeting {
                        text: format!("unreachable {missing}"),
                    })
                },
            ))
            .expect("unique route")
            .route(Route::get(
                "/challenge",
                |ctx: HttpContext<State>, _: ()| async move {
                    ctx.set_response_header("DPoP-Nonce", "n-err");
                    ctx.set_response_header("WWW-Authenticate", r#"DPoP error="use_dpop_nonce""#);
                    let missing: u64 = ctx.param("id")?;
                    Ok(Greeting {
                        text: format!("unreachable {missing}"),
                    })
                },
            ))
            .expect("unique route")
            .route(Route::post(
                "/no-content",
                |ctx: HttpContext<State>, _: ()| async move {
                    ctx.set_response_status(204);
                    Ok(())
                },
            ))
            .expect("unique route")
            .route(Route::post(
                "/accepted",
                |ctx: HttpContext<State>, _: ()| async move {
                    ctx.set_response_status(202);
                    Ok(Greeting {
                        text: "accepted".to_owned(),
                    })
                },
            ))
            .expect("unique route")
            .route(Route::get(
                "/status-then-error",
                |ctx: HttpContext<State>, _: ()| async move {
                    ctx.set_response_status(202);
                    let missing: u64 = ctx.param("id")?;
                    Ok(Greeting {
                        text: format!("unreachable {missing}"),
                    })
                },
            ))
            .expect("unique route")
            .route(Route::post(
                "/submit",
                |ctx: HttpContext<State>, _: ()| async move {
                    ctx.set_response_header("Set-Cookie", "__Host-session=t; Path=/");
                    ctx.redirect(Redirect::see_other("/welcome")?);
                    Ok(Greeting {
                        text: "signed in".to_owned(),
                    })
                },
            ))
            .expect("unique route")
            .route(Route::post(
                "/refuse",
                |ctx: HttpContext<State>, _: ()| async move {
                    ctx.redirect(Redirect::see_other("/submit?e=1")?);
                    let missing: u64 = ctx.param("id")?;
                    Ok(Greeting {
                        text: format!("unreachable {missing}"),
                    })
                },
            ))
            .expect("unique route")
            .route(Route::post(
                "/twice",
                |ctx: HttpContext<State>, _: ()| async move {
                    ctx.redirect(Redirect::see_other("/first")?);
                    ctx.redirect(Redirect::see_other("/second")?);
                    Ok(Greeting {
                        text: "ok".to_owned(),
                    })
                },
            ))
            .expect("unique route")
    }

    fn request(method: Method, path: &str) -> ServerRequest<State> {
        ServerRequest::new(method, path, State { greeting: "Hello" })
    }

    fn principal() -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            user_id: UserId::new("u1"),
            device_id: None,
            scopes: Vec::new(),
            expires_at: 0,
        }
    }

    #[tokio::test]
    async fn dispatches_to_the_matching_handler() {
        let response = router()
            .dispatch(request(Method::Post, "/greet").with_body(br#"{"name":"Ada"}"#.to_vec()))
            .await;
        assert_eq!(response.status(), 200);
        let greeting: Greeting = serde_json::from_slice(response.body()).expect("json");
        assert_eq!(greeting.text, "Hello, Ada");
    }

    #[tokio::test]
    async fn a_handler_set_response_header_rides_the_success_response() {
        let response = router().dispatch(request(Method::Get, "/nonce")).await;
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers(),
            &[("DPoP-Nonce".to_owned(), "n-ok".to_owned())]
        );
    }

    #[tokio::test]
    async fn a_handler_set_content_type_replaces_the_route_s_once() {
        let response = router().dispatch(request(Method::Get, "/file")).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.content_type(), "application/pdf");
        assert!(
            response
                .headers()
                .iter()
                .all(|(name, _)| !name.eq_ignore_ascii_case("content-type")),
            "{:?}",
            response.headers()
        );
    }

    #[tokio::test]
    async fn a_handler_set_content_type_never_relabels_its_error() {
        let response = router()
            .dispatch(request(Method::Get, "/file-refused"))
            .await;
        assert_eq!(response.status(), 500);
        assert_ne!(response.content_type(), "application/pdf");
        assert!(
            response
                .headers()
                .iter()
                .all(|(name, _)| !name.eq_ignore_ascii_case("content-type")),
            "{:?}",
            response.headers()
        );
    }

    #[tokio::test]
    async fn a_handler_set_response_status_rides_the_success_response() {
        let response = router().dispatch(request(Method::Post, "/accepted")).await;
        assert_eq!(
            response.status(),
            202,
            "the handler opted into 202 Accepted"
        );
    }

    #[tokio::test]
    async fn a_204_carries_no_body_and_no_content_type() {
        let response = router()
            .dispatch(request(Method::Post, "/no-content"))
            .await;
        assert_eq!(response.status(), 204);
        assert!(response.body().is_empty(), "{:?}", response.body());
        assert_eq!(response.content_type(), "");
    }

    #[tokio::test]
    async fn an_unset_status_defaults_to_200() {
        let response = router()
            .dispatch(request(Method::Post, "/greet-none"))
            .await;
        assert_eq!(response.status(), 404);
        let ok = router().dispatch(request(Method::Get, "/nonce")).await;
        assert_eq!(
            ok.status(),
            200,
            "a handler that sets no status defaults to 200"
        );
    }

    #[tokio::test]
    async fn an_error_status_wins_over_a_set_success_status() {
        // The error's own status wins over the 202 hint.
        let response = router()
            .dispatch(request(Method::Get, "/status-then-error"))
            .await;
        assert_ne!(
            response.status(),
            202,
            "the success hint never overrides an error"
        );
        assert_eq!(response.status(), 500, "an error's own status wins");
    }

    #[tokio::test]
    async fn a_successful_handler_that_redirects_answers_the_redirect() {
        let response = router().dispatch(request(Method::Post, "/submit")).await;
        assert_eq!(response.status(), 303);
        assert!(
            response
                .headers()
                .contains(&("location".to_owned(), "/welcome".to_owned())),
            "the Location comes from the redirect: {:?}",
            response.headers()
        );
        assert!(
            response.body().is_empty(),
            "a redirect carries no body, not the handler's serialized output"
        );
        assert!(
            response
                .headers()
                .iter()
                .any(|(name, _)| name == "Set-Cookie"),
            "a header the handler recorded still rides the redirect — a sign-in \
             sets its session cookie on exactly this response"
        );
    }

    // Without this, a form rejection answers a JSON `4xx`: a broken page without scripting.
    #[tokio::test]
    async fn a_refusing_handler_that_redirects_answers_the_redirect_and_not_the_error() {
        let response = router().dispatch(request(Method::Post, "/refuse")).await;
        assert_eq!(
            response.status(),
            303,
            "the redirect is honoured on the error arm"
        );
        assert!(
            response
                .headers()
                .contains(&("location".to_owned(), "/submit?e=1".to_owned()))
        );
        assert!(
            response.body().is_empty(),
            "no error envelope reaches a browser that asked for a page"
        );
    }

    #[tokio::test]
    async fn a_status_hint_still_loses_to_an_error_while_a_redirect_does_not() {
        let hinted = router()
            .dispatch(request(Method::Get, "/status-then-error"))
            .await;
        let redirected = router().dispatch(request(Method::Post, "/refuse")).await;
        assert_eq!(hinted.status(), 500);
        assert_eq!(redirected.status(), 303);
    }

    #[tokio::test]
    async fn redirecting_twice_leaves_one_destination() {
        let response = router().dispatch(request(Method::Post, "/twice")).await;
        let locations: Vec<&String> = response
            .headers()
            .iter()
            .filter(|(name, _)| name == "location")
            .map(|(_, value)| value)
            .collect();
        assert_eq!(locations, [&"/second".to_owned()]);
    }

    #[tokio::test]
    async fn challenge_headers_ride_the_error_response() {
        // A 4xx challenge must still carry `DPoP-Nonce` and `WWW-Authenticate` (RFC 9449 §8).
        let response = router().dispatch(request(Method::Get, "/challenge")).await;
        assert_ne!(response.status(), 200);
        assert_eq!(
            response.headers(),
            &[
                ("DPoP-Nonce".to_owned(), "n-err".to_owned()),
                (
                    "WWW-Authenticate".to_owned(),
                    r#"DPoP error="use_dpop_nonce""#.to_owned()
                ),
            ]
        );
    }

    #[tokio::test]
    async fn serves_a_static_asset_with_its_content_type() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.js"), b"export default 1;").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/app_bg.wasm"), b"\0asm").unwrap();

        let router = Router::<State>::new()
            .static_dir("/assets", dir.path())
            .expect("loads assets");

        let js = router
            .dispatch(request(Method::Get, "/assets/app.js"))
            .await;
        assert_eq!(js.status(), 200);
        assert_eq!(js.content_type(), "text/javascript; charset=utf-8");
        assert_eq!(js.body(), b"export default 1;");

        let wasm = router
            .dispatch(request(Method::Get, "/assets/sub/app_bg.wasm"))
            .await;
        assert_eq!(wasm.status(), 200);
        assert_eq!(wasm.content_type(), "application/wasm");
    }

    #[tokio::test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn a_static_cache_policy_applies_by_longest_prefix_from_memory_and_disk() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("fonts")).unwrap();
        std::fs::write(dir.path().join("fonts/inter.woff2"), b"wOF2").unwrap();
        std::fs::write(dir.path().join("app.css"), b"body{}").unwrap();
        std::fs::write(dir.path().join("app.js"), b"export default 1;").unwrap();

        for source in [StaticSource::Memory, StaticSource::Disk] {
            let router = Router::<State>::new()
                .static_dir_from("/_f", dir.path(), source)
                .expect("loads assets")
                .static_cache_control("/_f/", "public, max-age=1")
                .static_cache_control("/_f/fonts/", "public, max-age=31536000, immutable")
                .static_cache_control("/_f/app.css", "public, max-age=600");
            let policy = |response: &ServerResponse| {
                response
                    .headers()
                    .iter()
                    .filter(|(name, _)| name == "cache-control")
                    .map(|(_, value)| value.clone())
                    .collect::<Vec<_>>()
            };
            let font = router
                .dispatch(request(Method::Get, "/_f/fonts/inter.woff2"))
                .await;
            assert_eq!(policy(&font), ["public, max-age=31536000, immutable"]);
            let css = router.dispatch(request(Method::Get, "/_f/app.css")).await;
            assert_eq!(policy(&css), ["public, max-age=600"]);
            let js = router.dispatch(request(Method::Get, "/_f/app.js")).await;
            assert_eq!(
                policy(&js),
                ["public, max-age=1"],
                "the shorter prefix still applies"
            );
        }
    }

    #[tokio::test]
    async fn a_static_asset_has_no_cache_policy_unless_one_is_declared() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.js"), b"export default 1;").unwrap();
        let router = Router::<State>::new()
            .static_dir("/_f", dir.path())
            .expect("loads assets")
            .static_cache_control("/_f/fonts/", "public, max-age=31536000, immutable");
        let js = router.dispatch(request(Method::Get, "/_f/app.js")).await;
        assert!(
            js.headers()
                .iter()
                .all(|(name, _)| !name.eq_ignore_ascii_case("cache-control")),
            "a wasm loader must never be kept past the page that matches it"
        );
    }

    fn addressed_dir() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("fonts")).unwrap();
        std::fs::write(dir.path().join("fonts/inter.woff2"), b"wOF2").unwrap();
        std::fs::write(
            dir.path().join("app.css"),
            b"@font-face{src:url(\"/_f/fonts/inter.woff2\")}",
        )
        .unwrap();
        std::fs::write(dir.path().join("app.js"), b"export default 1;").unwrap();
        dir
    }

    fn version_of(router: &Router<State>, path: &str) -> String {
        router
            .asset_addresses()
            .version(path)
            .map(ToString::to_string)
            .unwrap_or_else(|| panic!("{path} is versioned"))
    }

    #[tokio::test]
    async fn an_addressed_asset_is_immutable() {
        let dir = addressed_dir();
        let router = Router::<State>::new()
            .static_dir("/_f", dir.path())
            .expect("loads assets")
            .static_cache_control("/_f/", "no-cache");
        let version = version_of(&router, "/_f/app.js");
        let js = router
            .dispatch(request(Method::Get, "/_f/app.js").with_query(format!("v={version}")))
            .await;
        assert_eq!(js.status(), 200);
        assert_eq!(header(&js, "cache-control"), [IMMUTABLE]);
        assert_eq!(header(&js, "etag"), [format!("\"{version}\"").as_str()]);
        assert_eq!(
            router.asset_addresses().url("/_f/app.js").to_string(),
            format!("/_f/app.js?v={version}")
        );
    }

    // Broken once on purpose: comparing only a prefix of the query made the padded version immutable.
    #[tokio::test]
    async fn a_stale_forged_or_padded_version_is_never_immutable() {
        let dir = addressed_dir();
        let router = Router::<State>::new()
            .static_dir("/_f", dir.path())
            .expect("loads assets")
            .static_cache_control("/_f/", "public, max-age=600");
        let version = version_of(&router, "/_f/app.js");
        let stale = version_of(&router, "/_f/app.css");
        for query in [
            format!("v={stale}"),
            "v=x".to_owned(),
            format!("v={version}&a=1"),
            format!("a=1&v={version}"),
            format!("v={}", &version[..42]),
        ] {
            let js = router
                .dispatch(request(Method::Get, "/_f/app.js").with_query(query.clone()))
                .await;
            assert_eq!(js.status(), 200, "{query}");
            assert_eq!(js.body(), b"export default 1;", "current bytes for {query}");
            assert_eq!(header(&js, "cache-control"), [REVALIDATE], "{query}");
        }
        let bare = router.dispatch(request(Method::Get, "/_f/app.js")).await;
        assert_eq!(header(&bare, "cache-control"), ["public, max-age=600"]);
    }

    #[tokio::test]
    async fn every_memory_asset_carries_a_strong_etag_and_if_none_match_is_a_304_with_no_body() {
        let dir = addressed_dir();
        let router = Router::<State>::new()
            .static_dir("/_f", dir.path())
            .expect("loads assets")
            .static_cache_control("/_f/", "no-cache");
        let version = version_of(&router, "/_f/app.js");
        let etag = format!("\"{version}\"");
        let conditional = |value: &str| {
            request(Method::Get, "/_f/app.js").with_headers(
                [("if-none-match".to_owned(), value.to_owned())]
                    .into_iter()
                    .collect(),
            )
        };
        let unchanged = router.dispatch(conditional(&etag)).await;
        assert_eq!(unchanged.status(), 304);
        assert!(unchanged.body().is_empty());
        assert_eq!(header(&unchanged, "etag"), [etag.as_str()]);
        assert_eq!(header(&unchanged, "cache-control"), ["no-cache"]);
        let changed = router.dispatch(conditional("\"old\"")).await;
        assert_eq!(changed.status(), 200);
        assert_eq!(changed.body(), b"export default 1;");
        assert_eq!(header(&changed, "etag"), [etag.as_str()]);
    }

    #[tokio::test]
    async fn live_reload_is_mounted_only_when_asked() {
        let dir = addressed_dir();
        let router = Router::<State>::new()
            .static_dir_from("/_f", dir.path(), StaticSource::Disk)
            .expect("mounts the directory")
            .live_reload_when(false)
            .expect("nothing to mount");
        assert!(!router.serves_live_reload());
        let live = router
            .dispatch(request(Method::Get, "/_fundamental/live"))
            .await;
        assert_eq!(live.status(), 404);
    }

    #[tokio::test]
    async fn live_reload_streams_events_from_hello() {
        let dir = addressed_dir();
        let router = Router::<State>::new()
            .static_dir_from("/_f", dir.path(), StaticSource::Disk)
            .expect("mounts the directory")
            .live_reload()
            .expect("mounts the stream");
        assert!(router.serves_live_reload());
        let mut live = router
            .dispatch(request(Method::Get, "/_fundamental/live"))
            .await;
        assert_eq!(live.status(), 200);
        assert_eq!(live.content_type(), "text/event-stream");
        let mut stream = live.take_stream().expect("streamed").into_stream();
        let first = std::future::poll_fn(|cx| futures_core::Stream::poll_next(stream.as_mut(), cx))
            .await
            .expect("an event")
            .expect("no error");
        assert!(first.starts_with(b"event: hello\ndata: "), "{first:?}");
    }

    #[tokio::test]
    async fn a_handler_on_a_live_reloading_router_knows_it() {
        let dir = addressed_dir();
        let router = Router::<State>::new()
            .route(Route::get(
                "/seen",
                |ctx: HttpContext<State>, _: ()| async move { Ok(ctx.serves_live_reload()) },
            ))
            .expect("unique route")
            .static_dir_from("/_f", dir.path(), StaticSource::Disk)
            .expect("mounts the directory")
            .live_reload()
            .expect("mounts the stream");
        let seen = router.dispatch(request(Method::Get, "/seen")).await;
        assert_eq!(seen.body(), b"true");
    }

    #[tokio::test]
    async fn ending_the_open_streams_ends_an_open_page_s_live_reload() {
        let dir = addressed_dir();
        let router = Router::<State>::new()
            .static_dir_from("/_f", dir.path(), StaticSource::Disk)
            .expect("mounts the directory")
            .live_reload()
            .expect("mounts the stream");
        let mut live = router
            .dispatch(request(Method::Get, "/_fundamental/live"))
            .await;
        let mut stream = live.take_stream().expect("streamed").into_stream();
        let hello =
            std::future::poll_fn(|cx| futures_core::Stream::poll_next(stream.as_mut(), cx)).await;
        assert!(hello.is_some(), "hello first");
        router.end_open_streams();
        let ended = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            std::future::poll_fn(|cx| futures_core::Stream::poll_next(stream.as_mut(), cx)),
        )
        .await
        .expect("the stream ends at once rather than holding the server open");
        assert!(ended.is_none(), "{ended:?}");
    }

    #[tokio::test]
    async fn live_reload_serves_its_client() {
        let dir = addressed_dir();
        let router = Router::<State>::new()
            .static_dir_from("/_f", dir.path(), StaticSource::Disk)
            .expect("mounts the directory")
            .live_reload()
            .expect("mounts the stream");
        let script = router
            .dispatch(request(Method::Get, "/_fundamental/live.js"))
            .await;
        assert_eq!(script.status(), 200);
        assert!(script.content_type().contains("javascript"));
        assert!(String::from_utf8_lossy(script.body()).contains("EventSource"));
    }

    #[tokio::test]
    async fn live_reload_without_a_disk_mount_mounts_nothing() {
        let router = Router::<State>::new()
            .live_reload()
            .expect("nothing to mount");
        assert!(!router.serves_live_reload());
        let live = router
            .dispatch(request(Method::Get, "/_fundamental/live"))
            .await;
        assert_eq!(live.status(), 404);
    }

    #[tokio::test]
    async fn a_disk_served_asset_is_never_immutable() {
        let dir = addressed_dir();
        let router = Router::<State>::new()
            .static_dir_from("/_f", dir.path(), StaticSource::Disk)
            .expect("mounts the directory");
        assert!(
            router.asset_addresses().is_empty(),
            "a file that changes under its path is never linked by a version"
        );
        let held = StaticAsset::new(Path::new("app.js"), b"export default 1;".to_vec());
        let version = held.version().expect("versioned").to_string();
        let js = router
            .dispatch(request(Method::Get, "/_f/app.js").with_query(format!("v={version}")))
            .await;
        assert_eq!(js.status(), 200);
        assert_eq!(header(&js, "etag"), [format!("\"{version}\"").as_str()]);
        assert_ne!(header(&js, "cache-control"), [IMMUTABLE]);
        assert!(
            router
                .addressed_answer("/_f/app.js", &format!("v={version}"), None, None)
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_addressed_document_answers_like_an_asset() {
        let router = Router::<State>::new()
            .document("/robots.txt", robots())
            .expect("a free path");
        let version = version_of(&router, "/robots.txt");
        let bare = router.dispatch(request(Method::Get, "/robots.txt")).await;
        assert_eq!(header(&bare, "cache-control"), ["public, max-age=3600"]);
        let addressed = router
            .dispatch(request(Method::Get, "/robots.txt").with_query(format!("v={version}")))
            .await;
        assert_eq!(header(&addressed, "cache-control"), [IMMUTABLE]);
        let scoped = Router::<State>::new()
            .document_on(
                WebOrigin::parse("https://example.com").expect("an origin"),
                "/sitemap.xml",
                robots(),
            )
            .expect("a free path");
        assert!(
            scoped.asset_addresses().version("/sitemap.xml").is_none(),
            "one origin's document is never linked"
        );
    }

    #[tokio::test]
    async fn a_loaded_stylesheet_names_its_fonts_by_version() {
        let dir = addressed_dir();
        let router = Router::<State>::new()
            .static_dir("/_f", dir.path())
            .expect("loads assets");
        let font = version_of(&router, "/_f/fonts/inter.woff2");
        let css = router.dispatch(request(Method::Get, "/_f/app.css")).await;
        let body = String::from_utf8(css.body().to_vec()).expect("utf-8");
        assert_eq!(
            body,
            format!("@font-face{{src:url(\"/_f/fonts/inter.woff2?v={font}\")}}")
        );
        let served = StaticAsset::new(Path::new("app.css"), body.into_bytes());
        assert_eq!(
            served.version().map(ToString::to_string),
            Some(version_of(&router, "/_f/app.css")),
            "the stylesheet's version covers the fonts it names"
        );
    }

    #[tokio::test]
    async fn only_a_declared_exact_match_is_answered_before_admission() {
        let dir = addressed_dir();
        let router = Router::<State>::new()
            .static_dir("/_f", dir.path())
            .expect("loads assets")
            .before_admission("/_f/app.css")
            .expect("served")
            .before_admission("/_f/fonts/")
            .expect("served");
        let css = version_of(&router, "/_f/app.css");
        let font = version_of(&router, "/_f/fonts/inter.woff2");
        let js = version_of(&router, "/_f/app.js");
        let early = router
            .addressed_answer("/_f/app.css", &format!("v={css}"), None, None)
            .expect("declared and addressed");
        assert_eq!(header(&early, "cache-control"), [IMMUTABLE]);
        assert!(
            router
                .addressed_answer("/_f/fonts/inter.woff2", &format!("v={font}"), None, None)
                .is_some()
        );
        assert!(
            router
                .addressed_answer("/_f/app.js", &format!("v={js}"), None, None)
                .is_none(),
            "an undeclared file meets admission however it is addressed"
        );
        assert!(
            router
                .addressed_answer("/_f/app.css", "", None, None)
                .is_none()
        );
        assert!(
            router
                .addressed_answer("/_f/app.css", &format!("v={js}"), None, None)
                .is_none()
        );
        let unchanged = router
            .addressed_answer(
                "/_f/app.css",
                &format!("v={css}"),
                None,
                Some(&format!("\"{css}\"")),
            )
            .expect("declared and addressed");
        assert_eq!(unchanged.status(), 304);
    }

    #[test]
    fn a_declaration_naming_nothing_served_is_refused() {
        let dir = addressed_dir();
        for path in ["/_f/missing.css", "/_f/nothing/", "/elsewhere"] {
            match Router::<State>::new()
                .static_dir("/_f", dir.path())
                .expect("loads assets")
                .before_admission(path)
            {
                Err(ServerError::NothingToAnswerEarly { path: refused }) => {
                    assert_eq!(refused, path);
                }
                Err(other) => panic!("expected API_032, got {}", other.error_code()),
                Ok(_) => panic!("{path} was accepted"),
            }
        }
    }

    #[tokio::test]
    async fn serves_an_in_memory_static_asset_at_its_exact_path() {
        use crate::StaticAsset;
        let router = Router::<State>::new().static_asset(
            "/_fundamental/runner.js",
            StaticAsset::new(
                std::path::Path::new("runner.js"),
                b"console.log(1)".to_vec(),
            ),
        );

        let served = router
            .dispatch(request(Method::Get, "/_fundamental/runner.js"))
            .await;
        assert_eq!(served.status(), 200);
        assert_eq!(served.content_type(), "text/javascript; charset=utf-8");
        assert_eq!(served.body(), b"console.log(1)");

        let posted = router
            .dispatch(request(Method::Post, "/_fundamental/runner.js"))
            .await;
        assert_eq!(posted.status(), 404);
    }

    fn robots() -> StaticAsset {
        StaticAsset::typed(b"User-agent: *\n".to_vec(), "text/plain; charset=utf-8")
            .with_cache_control("public, max-age=3600")
    }

    #[tokio::test]
    async fn a_document_is_served_before_anything_else_with_its_type_and_cache() {
        let router = Router::<State>::new()
            .document("/robots.txt", robots())
            .expect("a free path")
            .static_asset(
                "/robots.txt",
                StaticAsset::new(std::path::Path::new("robots.txt"), b"Disallow: /".to_vec()),
            );
        let served = router.dispatch(request(Method::Get, "/robots.txt")).await;
        assert_eq!(served.status(), 200);
        assert_eq!(served.content_type(), "text/plain; charset=utf-8");
        assert_eq!(served.body(), b"User-agent: *\n");
        assert!(
            served
                .headers()
                .iter()
                .any(|(name, value)| name == "cache-control" && value == "public, max-age=3600")
        );
    }

    #[tokio::test]
    async fn a_document_refuses_a_path_an_asset_a_route_or_a_document_holds() {
        let held_by = |result: ServerResult<Router<State>>| match result {
            Err(ServerError::PathTaken { held_by, .. }) => held_by,
            Err(other) => panic!("expected API_019, got {}", other.error_code()),
            Ok(_) => panic!("expected API_019, got a router"),
        };
        let asset = Router::<State>::new().static_asset("/robots.txt", robots());
        assert_eq!(held_by(asset.document("/robots.txt", robots())), "an asset");
        let routed = router()
            .route(Route::<State>::html("/robots.txt", |_| async move {
                Ok(String::new())
            }))
            .expect("a route");
        assert_eq!(held_by(routed.document("/robots.txt", robots())), "a route");
        let twice = Router::<State>::new()
            .document("/robots.txt", robots())
            .expect("first");
        assert_eq!(
            held_by(twice.document("/robots.txt", robots())),
            "another document"
        );
        let documented = Router::<State>::new()
            .document("/robots.txt", robots())
            .expect("first");
        let error = documented
            .route(Route::<State>::html("/robots.txt", |_| async move {
                Ok(String::new())
            }))
            .err()
            .expect("a route at a document's path");
        assert_eq!(error.error_code(), "API_019");
    }

    #[tokio::test]
    async fn an_asset_directory_cannot_shadow_a_document() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("robots.txt"), b"Disallow: /").expect("write");
        let error = Router::<State>::new()
            .document("/robots.txt", robots())
            .expect("a free path")
            .static_dir("/", dir.path())
            .err()
            .expect("a stale file at the document's path");
        assert_eq!(error.error_code(), "API_019");
    }

    fn two_origin_router() -> Router<State> {
        use fundamental_core::address::{AddressSpace, LocalePrefixSet, SlugTable};
        let origin = |value: &str| WebOrigin::parse(value).expect("origin");
        let space = AddressSpace::new(LocalePrefixSet::new(), SlugTable::default())
            .with_language_origins(
                [
                    (
                        "nl".parse().expect("tag"),
                        origin("https://shop.example.nl"),
                    ),
                    (
                        "en".parse().expect("tag"),
                        origin("https://shop.example.com"),
                    ),
                ],
                &"en".parse().expect("tag"),
            )
            .expect("origins")
            .with_alias(
                origin("https://shop.example.org"),
                origin("https://shop.example.nl"),
            )
            .expect("alias");
        Router::<State>::new()
            .with_addresses(Arc::new(space))
            .route(Route::<State>::html("/prijzen", |ctx| async move {
                Ok(ctx
                    .address()
                    .map(|address| address.locale().to_string())
                    .unwrap_or_default())
            }))
            .expect("a page")
            .document_on(
                origin("https://shop.example.nl"),
                "/robots.txt",
                StaticAsset::typed(b"nl".to_vec(), "text/plain; charset=utf-8"),
            )
            .expect("nl robots")
            .document_on(
                origin("https://shop.example.com"),
                "/robots.txt",
                StaticAsset::typed(b"en".to_vec(), "text/plain; charset=utf-8"),
            )
            .expect("en robots")
    }

    fn addressed_to(method: Method, path: &str, authority: &str) -> ServerRequest<State> {
        request(method, path).with_authority(Some(authority.to_owned()))
    }

    #[tokio::test]
    async fn an_alias_answers_with_the_same_path_and_query_on_its_origin() {
        let router = two_origin_router();
        let get = router
            .dispatch(addressed_to(Method::Get, "/prijzen", "shop.example.org").with_query("a=1"))
            .await;
        assert_eq!(get.status(), 301);
        assert!(
            get.headers().iter().any(|(name, value)| name == "location"
                && value == "https://shop.example.nl/prijzen?a=1")
        );
        let post = router
            .dispatch(addressed_to(Method::Post, "/prijzen", "shop.example.org"))
            .await;
        assert_eq!(post.status(), 308, "a write keeps its method and body");
        let served = router
            .dispatch(addressed_to(Method::Get, "/prijzen", "shop.example.nl"))
            .await;
        assert_eq!(served.status(), 200, "the origin itself is not redirected");
    }

    #[tokio::test]
    async fn each_origin_has_its_own_document_and_its_own_language() {
        let router = two_origin_router();
        for (authority, expected) in [("shop.example.nl", "nl"), ("shop.example.com", "en")] {
            let robots = router
                .dispatch(addressed_to(Method::Get, "/robots.txt", authority))
                .await;
            assert_eq!(robots.body(), expected.as_bytes(), "{authority}");
            let page = router
                .dispatch(addressed_to(Method::Get, "/prijzen", authority))
                .await;
            assert_eq!(page.body(), expected.as_bytes(), "{authority}");
        }
        let forged = router
            .dispatch(addressed_to(Method::Get, "/robots.txt", "evil.example"))
            .await;
        assert_eq!(
            forged.status(),
            404,
            "no document answers an origin nobody published"
        );
    }

    #[test]
    fn a_disk_mount_cannot_take_a_documents_path() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let error = Router::<State>::new()
            .document("/robots.txt", robots())
            .expect("a free path")
            .static_dir_from("/", dir.path(), StaticSource::Disk)
            .err()
            .expect("a disk mount over a document");
        assert_eq!(error.error_code(), "API_019");
    }

    #[test]
    fn serves_page_answers_only_for_an_html_get_route() {
        let router = router()
            .route(Route::<State>::html("/guides/{slug}", |_| async move {
                Ok(String::new())
            }))
            .expect("a page");
        assert!(router.serves_page("/guides/tax"));
        assert!(
            !router.serves_page("/users/1"),
            "a JSON route is not a page"
        );
        assert!(!router.serves_page("/greet"), "a POST is not a page");
        assert!(!router.serves_page("/nowhere"));
    }

    #[tokio::test]
    async fn missing_static_asset_falls_through_to_404() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.js"), b"x").unwrap();
        let router = Router::<State>::new()
            .static_dir("/assets", dir.path())
            .expect("loads assets");
        let response = router
            .dispatch(request(Method::Get, "/assets/missing.js"))
            .await;
        assert_eq!(response.status(), 404);
    }

    #[tokio::test]
    async fn a_disk_mount_serves_a_file_written_after_construction() {
        let dir = tempfile::tempdir().expect("tempdir");
        let router = Router::<State>::new()
            .static_dir_from("/assets", dir.path(), StaticSource::Disk)
            .expect("registers");

        let missing = router
            .dispatch(request(Method::Get, "/assets/app.js"))
            .await;
        assert_eq!(missing.status(), 404);

        std::fs::write(dir.path().join("app.js"), b"export const rebuilt = 1;").expect("write");

        let served = router
            .dispatch(request(Method::Get, "/assets/app.js"))
            .await;
        assert_eq!(served.status(), 200);
        assert_eq!(served.body(), b"export const rebuilt = 1;");
    }

    #[tokio::test]
    async fn a_disk_hit_records_the_asset_key_and_not_the_padded_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("app.js"), b"export {};").expect("write");
        let router = Router::<State>::new()
            .static_dir_from("/assets", dir.path(), StaticSource::Disk)
            .expect("registers");
        let padded = format!("/assets/{}app.js", "./".repeat(200));
        let (status, logs) = logs_of(&router, request(Method::Get, &padded)).await;
        assert_eq!(status, 200);
        assert!(logs.contains("path=/assets/app.js"), "{logs}");
        assert!(!logs.contains("././"), "{logs}");
    }

    #[tokio::test]
    async fn a_memory_mount_does_not_see_a_later_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("app.js"), b"old").expect("write");
        let router = Router::<State>::new()
            .static_dir_from("/assets", dir.path(), StaticSource::Memory)
            .expect("registers");

        std::fs::write(dir.path().join("app.js"), b"new").expect("rewrite");
        let served = router
            .dispatch(request(Method::Get, "/assets/app.js"))
            .await;
        assert_eq!(served.body(), b"old", "memory serving must not re-read");
    }

    #[tokio::test]
    async fn a_disk_mount_refuses_every_path_outside_it() {
        let root = tempfile::tempdir().expect("tempdir");
        let mount = root.path().join("dist");
        std::fs::create_dir_all(&mount).expect("dist");
        std::fs::write(mount.join("app.js"), b"ok").expect("inside");
        std::fs::write(root.path().join("secret.pem"), b"KEY").expect("outside");

        let router = Router::<State>::new()
            .static_dir_from("/assets", &mount, StaticSource::Disk)
            .expect("registers");

        for attempt in [
            "/assets/../secret.pem",
            "/assets/../../secret.pem",
            "/assets/./../secret.pem",
            "/assets//../secret.pem",
            "/assets/sub/../../secret.pem",
        ] {
            let response = router.dispatch(request(Method::Get, attempt)).await;
            assert_eq!(response.status(), 404, "leaked via {attempt}");
            assert_ne!(response.body(), b"KEY", "leaked via {attempt}");
        }

        // The legitimate file still works, so the guard is not simply refusing all.
        let ok = router
            .dispatch(request(Method::Get, "/assets/app.js"))
            .await;
        assert_eq!(ok.status(), 200);
    }

    // Canonicalisation, not the `..` check, is what refuses this.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_disk_mount_refuses_a_symlink_that_escapes() {
        let root = tempfile::tempdir().expect("tempdir");
        let mount = root.path().join("dist");
        std::fs::create_dir_all(&mount).expect("dist");
        std::fs::write(root.path().join("secret.pem"), b"KEY").expect("outside");
        std::os::unix::fs::symlink(root.path().join("secret.pem"), mount.join("link.pem"))
            .expect("symlink");

        let router = Router::<State>::new()
            .static_dir_from("/assets", &mount, StaticSource::Disk)
            .expect("registers");
        let response = router
            .dispatch(request(Method::Get, "/assets/link.pem"))
            .await;
        assert_eq!(response.status(), 404);
        assert_ne!(response.body(), b"KEY");
    }

    /// No directory listing: a mount exists to serve named files.
    #[tokio::test]
    async fn a_disk_mount_does_not_list_a_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("app.js"), b"ok").expect("write");
        let router = Router::<State>::new()
            .static_dir_from("/assets", dir.path(), StaticSource::Disk)
            .expect("registers");

        for path in ["/assets", "/assets/"] {
            let response = router.dispatch(request(Method::Get, path)).await;
            assert_eq!(response.status(), 404, "listed via {path}");
        }
    }

    /// Only a `GET` reads an asset, on disk exactly as in memory.
    #[tokio::test]
    async fn a_disk_mount_ignores_a_non_get() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("app.js"), b"ok").expect("write");
        let router = Router::<State>::new()
            .static_dir_from("/assets", dir.path(), StaticSource::Disk)
            .expect("registers");
        let response = router
            .dispatch(request(Method::Post, "/assets/app.js"))
            .await;
        assert_ne!(response.status(), 200);
    }

    #[tokio::test]
    async fn absent_static_dir_registers_nothing() {
        let router = Router::<State>::new()
            .static_dir("/assets", "/no/such/dir")
            .expect("absent dir is ok");
        let response = router
            .dispatch(request(Method::Get, "/assets/app.js"))
            .await;
        assert_eq!(response.status(), 404);
    }

    #[tokio::test]
    async fn html_route_renders_with_a_text_html_content_type() {
        let router = Router::new()
            .route(Route::<State>::html("/page", |ctx| async move {
                Ok(format!("<h1>{}</h1>", ctx.state().greeting))
            }))
            .expect("unique route");
        let response = router.dispatch(request(Method::Get, "/page")).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.content_type(), "text/html; charset=utf-8");
        assert_eq!(response.body(), b"<h1>Hello</h1>");
    }

    #[tokio::test]
    async fn json_route_keeps_its_application_json_content_type() {
        let response = router().dispatch(request(Method::Get, "/users/7")).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.content_type(), "application/json");
    }

    #[tokio::test]
    async fn captures_path_params() {
        let response = router().dispatch(request(Method::Get, "/users/42")).await;
        assert_eq!(response.status(), 200);
        let greeting: Greeting = serde_json::from_slice(response.body()).expect("json");
        assert_eq!(greeting.text, "user 42");
    }

    #[tokio::test]
    async fn unknown_path_is_404() {
        let response = router().dispatch(request(Method::Get, "/nope")).await;
        assert_eq!(response.status(), 404);
        let value: serde_json::Value = serde_json::from_slice(response.body()).expect("json");
        assert_eq!(value["error"], "API_002");
    }

    #[tokio::test]
    async fn a_404_names_the_shape_of_the_path_never_its_values() {
        let response = router()
            .dispatch(request(Method::Get, "/users/42/jan@example.nl"))
            .await;
        assert_eq!(response.status(), 404);
        let body = String::from_utf8_lossy(response.body()).into_owned();
        assert!(body.contains("/users/{number:1-8}/{email}"), "{body}");
        assert!(!body.contains("jan"), "{body}");
    }

    #[tokio::test]
    async fn a_405_names_the_shape_of_the_path_never_its_values() {
        let response = router()
            .dispatch(request(Method::Delete, "/users/0612345678"))
            .await;
        assert_eq!(response.status(), 405);
        let body = String::from_utf8_lossy(response.body()).into_owned();
        assert!(body.contains("/users/{number:9-16}"), "{body}");
        assert!(!body.contains("0612345678"), "{body}");
    }

    #[tokio::test]
    async fn wrong_method_is_405() {
        let response = router().dispatch(request(Method::Delete, "/greet")).await;
        assert_eq!(response.status(), 405);
        let value: serde_json::Value = serde_json::from_slice(response.body()).expect("json");
        assert_eq!(value["error"], "API_004");
    }

    #[tokio::test]
    async fn a_405_names_the_methods_that_would_have_worked() {
        let response = router().dispatch(request(Method::Delete, "/greet")).await;
        let allow = response
            .headers()
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("allow"))
            .map(|(_, value)| value.clone())
            .expect("a 405 must carry Allow");
        assert!(
            allow.contains("POST"),
            "`/greet` is a POST route and the refusal must say so: {allow}"
        );
        assert!(
            !allow.contains("DELETE"),
            "the method that was just refused cannot be in the allowed set: {allow}"
        );
    }

    #[tokio::test]
    async fn allow_lists_head_wherever_it_lists_get() {
        let response = router()
            .dispatch(request(Method::Delete, "/users/42"))
            .await;
        assert_eq!(response.status(), 405);
        let allow = response
            .headers()
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("allow"))
            .map(|(_, value)| value.clone())
            .expect("a 405 must carry Allow");
        assert!(allow.contains("GET"), "{allow}");
        assert!(
            allow.contains("HEAD"),
            "`HEAD` is served wherever `GET` is, so it belongs in the set: {allow}"
        );
    }

    // An empty `Allow` would read as a resource that permits no method.
    #[tokio::test]
    async fn a_404_carries_no_allow() {
        let response = router().dispatch(request(Method::Get, "/nope")).await;
        assert_eq!(response.status(), 404);
        assert!(
            !response
                .headers()
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("allow")),
            "a path that matched nothing has no method set to advertise"
        );
    }

    #[tokio::test]
    async fn malformed_body_is_400() {
        let response = router()
            .dispatch(request(Method::Post, "/greet").with_body(br#"{"name":5}"#.to_vec()))
            .await;
        assert_eq!(response.status(), 400);
        let value: serde_json::Value = serde_json::from_slice(response.body()).expect("json");
        assert_eq!(value["error"], "API_007");
    }

    #[tokio::test]
    async fn auth_required_without_principal_is_401() {
        let response = router().dispatch(request(Method::Get, "/me")).await;
        assert_eq!(response.status(), 401);
        let value: serde_json::Value = serde_json::from_slice(response.body()).expect("json");
        assert_eq!(value["error"], "API_009");
    }

    #[tokio::test]
    async fn auth_required_with_principal_runs() {
        let response = router()
            .dispatch(request(Method::Get, "/me").with_principal(Some(principal())))
            .await;
        assert_eq!(response.status(), 200);
    }

    #[test]
    fn duplicate_route_is_a_conflict() {
        let first = Router::new()
            .route(Route::<State>::get(
                "/x",
                |_ctx, _: ()| async move { Ok(()) },
            ))
            .expect("first route");
        // `Router` is not `Debug`, so match rather than `expect_err`.
        let err = match first.route(Route::<State>::get(
            "/x",
            |_ctx, _: ()| async move { Ok(()) },
        )) {
            Ok(_) => panic!("expected a route conflict"),
            Err(error) => error,
        };
        assert_eq!(err.error_code(), "API_001");
        assert!(matches!(err, ServerError::RouteConflict { .. }));
    }

    /// Records its label when it runs, then continues the chain.
    struct Mark {
        label: &'static str,
        seen: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
    }

    impl Middleware<State> for Mark {
        fn handle(
            &self,
            ctx: HttpContext<State>,
            body: Vec<u8>,
            next: crate::Next<'_, State>,
        ) -> impl std::future::Future<Output = crate::MiddlewareResult> + Send {
            self.seen
                .lock()
                .expect("the recorder is not poisoned")
                .push(self.label);
            next.run(ctx, body)
        }
    }

    fn ok_route(path: &str) -> Route<State> {
        Route::<State>::get(path, |_ctx, _: ()| async move { Ok(()) })
    }

    #[tokio::test]
    async fn router_middleware_covers_routes_registered_before_and_after_it() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let router = Router::new()
            .route(ok_route("/before"))
            .expect("first route")
            .middleware(Mark {
                label: "router",
                seen: seen.clone(),
            })
            .route(ok_route("/after"))
            .expect("second route");

        router.dispatch(request(Method::Get, "/before")).await;
        router.dispatch(request(Method::Get, "/after")).await;

        assert_eq!(
            seen.lock()
                .expect("the recorder is not poisoned")
                .as_slice(),
            ["router", "router"]
        );
    }

    #[tokio::test]
    async fn router_middleware_runs_outside_the_routes_own_chain() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let router = Router::new()
            .middleware(Mark {
                label: "router",
                seen: seen.clone(),
            })
            .route(ok_route("/x").middleware(Mark {
                label: "route",
                seen: seen.clone(),
            }))
            .expect("the route registers");

        router.dispatch(request(Method::Get, "/x")).await;

        assert_eq!(
            seen.lock()
                .expect("the recorder is not poisoned")
                .as_slice(),
            ["router", "route"]
        );
    }

    #[tokio::test]
    async fn router_middleware_does_not_see_an_unmatched_request() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let router = Router::new()
            .middleware(Mark {
                label: "router",
                seen: seen.clone(),
            })
            .route(ok_route("/x"))
            .expect("the route registers");

        let response = router.dispatch(request(Method::Get, "/nowhere")).await;

        assert_eq!(response.status(), 404);
        assert!(
            seen.lock()
                .expect("the recorder is not poisoned")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_request_carries_its_route_template_not_its_path() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        struct CaptureTemplate(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        impl Middleware<State> for CaptureTemplate {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn handle(
                &self,
                ctx: HttpContext<State>,
                body: Vec<u8>,
                next: crate::Next<'_, State>,
            ) -> impl std::future::Future<Output = crate::MiddlewareResult> + Send {
                self.0
                    .lock()
                    .expect("the recorder is not poisoned")
                    .push(ctx.route_template().unwrap_or_default().to_string());
                next.run(ctx, body)
            }
        }

        let router = Router::new()
            .middleware(CaptureTemplate(seen.clone()))
            .route(ok_route("/users/{id}"))
            .expect("the route registers");

        router.dispatch(request(Method::Get, "/users/42")).await;
        router.dispatch(request(Method::Get, "/users/1337")).await;

        assert_eq!(
            seen.lock()
                .expect("the recorder is not poisoned")
                .as_slice(),
            ["/users/{id}", "/users/{id}"],
            "two requests, one label value"
        );
    }

    /// A well-formed W3C header naming a trace this process did not start.
    const UPSTREAM: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    const UPSTREAM_TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";

    /// Dispatches `GET /users/42` with `headers` under `policy`, answering the handler's trace id and state.
    async fn trace_seen_by_the_handler(
        policy: Option<InboundTraceContext>,
        headers: &[(&str, &str)],
    ) -> (String, String) {
        use opentelemetry::trace::TracerProvider as _;
        use tracing::instrument::WithSubscriber;
        use tracing_subscriber::layer::SubscriberExt;

        // If the dev-dependency feature stopped reaching the build, adoption would be a silent no-op.
        assert!(
            fundamental_observability::trace_context::is_propagating(),
            "this test proves propagation, so the build must carry it"
        );

        type Seen = std::sync::Arc<std::sync::Mutex<Option<(String, String)>>>;
        let seen: Seen = std::sync::Arc::default();

        struct CaptureTrace(Seen);
        impl Middleware<State> for CaptureTrace {
            fn handle(
                &self,
                ctx: HttpContext<State>,
                body: Vec<u8>,
                next: crate::Next<'_, State>,
            ) -> impl std::future::Future<Output = crate::MiddlewareResult> + Send {
                use opentelemetry::trace::TraceContextExt;
                use tracing_opentelemetry::OpenTelemetrySpanExt;

                let context = tracing::Span::current().context();
                let span = context.span();
                let span_context = span.span_context();
                *self.0.lock().expect("the recorder is not poisoned") = Some((
                    span_context.trace_id().to_string(),
                    span_context.trace_state().header(),
                ));
                next.run(ctx, body)
            }
        }

        let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder().build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("test")));

        let router = Router::new()
            .middleware(CaptureTrace(seen.clone()))
            .route(ok_route("/users/{id}"))
            .expect("the route registers");

        let request = request(Method::Get, "/users/42").with_headers(
            headers
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        );
        let request = match policy {
            Some(policy) => request.with_inbound_trace_context(policy),
            None => request,
        };

        // Only one subscriber may be the global default in a test binary.
        let dispatched = router.dispatch(request).with_subscriber(subscriber).await;
        assert_eq!(dispatched.status(), 200);

        let seen = seen.lock().expect("the recorder is not poisoned").clone();
        seen.expect("the handler ran")
    }

    // A span's parent is fixed when entered, so `#[instrument]` could not adopt one.
    #[tokio::test]
    async fn a_trusted_caller_s_trace_is_continued() {
        let (trace, _) = trace_seen_by_the_handler(
            Some(InboundTraceContext::Adopt),
            &[("traceparent", UPSTREAM)],
        )
        .await;
        assert_eq!(
            trace, UPSTREAM_TRACE,
            "the handler must run inside the caller's trace"
        );
    }

    #[tokio::test]
    async fn a_trusted_caller_s_trace_state_is_never_adopted() {
        let (trace, state) = trace_seen_by_the_handler(
            Some(InboundTraceContext::Adopt),
            &[
                ("traceparent", UPSTREAM),
                ("tracestate", "x=jan@example.nl"),
            ],
        )
        .await;
        assert_eq!(trace, UPSTREAM_TRACE);
        assert_eq!(state, "", "no trace state is taken from a caller");
    }

    #[tokio::test]
    async fn a_linked_caller_s_trace_is_not_joined() {
        let (trace, _) = trace_seen_by_the_handler(
            Some(InboundTraceContext::default()),
            &[("traceparent", UPSTREAM)],
        )
        .await;
        assert_ne!(trace, UPSTREAM_TRACE);
    }

    /// A request no backend attached a policy to uses none of the caller's context.
    #[tokio::test]
    async fn without_a_policy_the_caller_s_trace_is_ignored() {
        let (trace, _) = trace_seen_by_the_handler(None, &[("traceparent", UPSTREAM)]).await;
        assert_ne!(trace, UPSTREAM_TRACE);
    }

    // OpenTelemetry's own extractor accepts this short trace id; the strict parse does not.
    #[tokio::test]
    async fn a_malformed_trace_context_is_not_adopted_even_from_a_trusted_caller() {
        let (trace, _) = trace_seen_by_the_handler(
            Some(InboundTraceContext::Adopt),
            &[(
                "traceparent",
                "00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01",
            )],
        )
        .await;
        assert_ne!(trace, "04bf92f3577b34da6a3ce929d0e0e473");
        assert_ne!(trace, UPSTREAM_TRACE);
    }

    #[test]
    fn principal_gated_routes_lists_only_the_gated_ones() {
        let router = Router::<State>::new()
            .route(Route::get("/health", |_ctx, _: ()| async move { Ok(()) }))
            .expect("unique route")
            .route(
                Route::post("/erase", |_ctx, _: ()| async move { Ok(()) }).middleware(crate::auth),
            )
            .expect("unique route");

        assert_eq!(
            router.principal_gated_routes(),
            vec![(Method::Post, "/erase")]
        );
    }

    #[test]
    fn a_cross_origin_route_names_its_origin_set() {
        let router = Router::<State>::new()
            .route(
                Route::post("/chat/{id}/messages", |_ctx, _: ()| async move { Ok(()) })
                    .cross_origin("site"),
            )
            .expect("unique route")
            .route(Route::post("/account", |_ctx, _: ()| async move { Ok(()) }))
            .expect("unique route");

        assert_eq!(
            router.cross_origin(Method::Post, "/chat/abc/messages"),
            Some("site")
        );
        assert_eq!(router.cross_origin(Method::Get, "/chat/abc/messages"), None);
        assert_eq!(router.cross_origin(Method::Post, "/account"), None);
        assert_eq!(router.cross_origin(Method::Post, "/nowhere"), None);
        assert_eq!(
            router.cross_origin_routes(),
            vec![(Method::Post, "/chat/{id}/messages", "site")]
        );
    }

    #[test]
    fn a_cross_origin_route_that_needs_a_principal_is_named() {
        let router = Router::<State>::new()
            .route(
                Route::post("/chat", |_ctx, _: ()| async move { Ok(()) })
                    .cross_origin("site")
                    .middleware(crate::auth),
            )
            .expect("unique route")
            .route(Route::post("/open", |_ctx, _: ()| async move { Ok(()) }).cross_origin("site"))
            .expect("unique route");

        assert_eq!(
            router.cross_origin_routes_needing_a_principal(),
            vec![(Method::Post, "/chat")]
        );
    }

    #[test]
    fn a_router_with_no_gated_routes_reports_none() {
        let router = Router::<State>::new()
            .route(Route::get("/health", |_ctx, _: ()| async move { Ok(()) }))
            .expect("unique route");
        assert!(router.principal_gated_routes().is_empty());
    }

    #[test]
    fn a_router_level_guard_gates_every_route() {
        let router = Router::<State>::new()
            .route(Route::get("/health", |_ctx, _: ()| async move { Ok(()) }))
            .expect("unique route")
            .route(Route::get("/me", |_ctx, _: ()| async move { Ok(()) }))
            .expect("unique route")
            .middleware(crate::auth);

        assert_eq!(
            router.principal_gated_routes(),
            vec![(Method::Get, "/health"), (Method::Get, "/me")]
        );
    }

    /// A layer that authorizes an object from the path, as `PermissionGuard<PathObject>` does.
    struct Authorizing;

    impl Middleware<State> for Authorizing {
        const GRANTS_AUTHORIZATION: bool = true;
        const AUTHORIZES_FROM_PATH: bool = true;

        fn handle(
            &self,
            ctx: HttpContext<State>,
            body: Vec<u8>,
            next: crate::Next<'_, State>,
        ) -> impl std::future::Future<Output = crate::MiddlewareResult> + Send {
            next.run(ctx, body)
        }
    }

    #[test]
    fn an_authenticated_write_that_authorizes_no_object_is_named() {
        let router = Router::<State>::new()
            .route(
                Route::post(
                    "/credentials/{id}/revoke",
                    |_ctx, _: ()| async move { Ok(()) },
                )
                .middleware(crate::auth),
            )
            .expect("unique route");

        assert_eq!(
            router.authenticated_unauthorized_writes(),
            vec![(Method::Post, "/credentials/{id}/revoke")]
        );
    }

    #[test]
    fn a_write_whose_chain_authorizes_an_object_is_not_named() {
        let router = Router::<State>::new()
            .route(
                Route::post(
                    "/credentials/{id}/revoke",
                    |_ctx, _: ()| async move { Ok(()) },
                )
                .middleware(crate::auth)
                .middleware(Authorizing),
            )
            .expect("unique route");

        assert!(router.authenticated_unauthorized_writes().is_empty());
    }

    // A read needing per-object authorization needs it for confidentiality, a different question.
    #[test]
    fn a_safe_method_is_never_named_however_it_is_guarded() {
        let router = Router::<State>::new()
            .route(Route::get("/me", |_ctx, _: ()| async move { Ok(()) }).middleware(crate::auth))
            .expect("unique route");

        assert!(router.authenticated_unauthorized_writes().is_empty());
    }

    #[test]
    fn a_write_no_principal_reaches_is_not_named() {
        let router = Router::<State>::new()
            .route(Route::post("/token", |_ctx, _: ()| async move { Ok(()) }))
            .expect("unique route");

        assert!(router.authenticated_unauthorized_writes().is_empty());
    }

    #[test]
    fn a_route_declaring_no_single_object_is_passed_over() {
        let router = Router::<State>::new()
            .route(
                Route::post("/erase", |_ctx, _: ()| async move { Ok(()) })
                    .middleware(crate::auth)
                    .no_single_object(),
            )
            .expect("unique route");

        assert!(router.authenticated_unauthorized_writes().is_empty());
    }

    #[test]
    fn a_router_level_authorizing_layer_covers_every_route() {
        let router = Router::<State>::new()
            .route(Route::post("/a", |_ctx, _: ()| async move { Ok(()) }))
            .expect("unique route")
            .middleware(crate::auth)
            .middleware(Authorizing);

        assert!(router.authenticated_unauthorized_writes().is_empty());
    }

    #[test]
    fn only_the_unguarded_writes_are_named() {
        let router = Router::<State>::new()
            .route(Route::get("/health", |_ctx, _: ()| async move { Ok(()) }))
            .expect("unique route")
            .route(
                Route::post("/guarded", |_ctx, _: ()| async move { Ok(()) })
                    .middleware(crate::auth)
                    .middleware(Authorizing),
            )
            .expect("unique route")
            .route(
                Route::post("/unguarded", |_ctx, _: ()| async move { Ok(()) })
                    .middleware(crate::auth),
            )
            .expect("unique route");

        assert_eq!(
            router.authenticated_unauthorized_writes(),
            vec![(Method::Post, "/unguarded")]
        );
    }

    /// A layer that authorizes an object it did not take from the path.
    struct AuthorizingElsewhere;

    impl Middleware<State> for AuthorizingElsewhere {
        const GRANTS_AUTHORIZATION: bool = true;

        fn handle(
            &self,
            ctx: HttpContext<State>,
            body: Vec<u8>,
            next: crate::Next<'_, State>,
        ) -> impl std::future::Future<Output = crate::MiddlewareResult> + Send {
            next.run(ctx, body)
        }
    }

    // The guard names a fixed object while the handler is handed `{id}`.
    #[test]
    fn a_parameterised_write_guarded_on_a_fixed_object_is_named() {
        let router = Router::<State>::new()
            .route(
                Route::post(
                    "/credentials/{id}/revoke",
                    |_ctx, _: ()| async move { Ok(()) },
                )
                .middleware(crate::auth)
                .middleware(AuthorizingElsewhere),
            )
            .expect("unique route");

        assert_eq!(
            router.authenticated_unauthorized_writes(),
            vec![(Method::Post, "/credentials/{id}/revoke")]
        );
    }

    #[test]
    fn a_write_that_captures_nothing_may_authorize_a_fixed_object() {
        let router = Router::<State>::new()
            .route(
                Route::post("/credential-offers", |_ctx, _: ()| async move { Ok(()) })
                    .middleware(crate::auth)
                    .middleware(AuthorizingElsewhere),
            )
            .expect("unique route");

        assert!(router.authenticated_unauthorized_writes().is_empty());
    }

    /// A catch-all captures too, so it is a resource path like any other.
    #[test]
    fn a_catch_all_write_guarded_on_a_fixed_object_is_named() {
        let router = Router::<State>::new()
            .route(
                Route::post("/files/{*path}", |_ctx, _: ()| async move { Ok(()) })
                    .middleware(crate::auth)
                    .middleware(AuthorizingElsewhere),
            )
            .expect("unique route");

        assert_eq!(
            router.authenticated_unauthorized_writes(),
            vec![(Method::Post, "/files/{*path}")]
        );
    }

    /// A router with a page for unmatched requests, plus one real route to 405 on.
    fn router_with_a_not_found_page() -> Router<State> {
        Router::<State>::new()
            .route(Route::html(
                "/here",
                |_ctx: HttpContext<State>| async move { Ok("<p>here</p>".to_owned()) },
            ))
            .expect("unique route")
            .not_found(Route::html(
                "__not_found",
                |_ctx: HttpContext<State>| async move { Ok("<h1>No such page</h1>".to_owned()) },
            ))
    }

    /// A request shaped like a browser navigation: it says it wants HTML.
    fn navigating(method: Method, path: &str) -> ServerRequest<State> {
        request(method, path).with_headers(
            [(
                "accept".to_owned(),
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".to_owned(),
            )]
            .into_iter()
            .collect(),
        )
    }

    fn navigation(path: &str) -> ServerRequest<State> {
        navigating(Method::Get, path)
    }

    #[tokio::test]
    async fn an_unmatched_navigation_gets_the_not_found_page() {
        let response = router_with_a_not_found_page()
            .dispatch(navigation("/nope"))
            .await;
        assert_eq!(response.status(), 404, "the page answers 404 by default");
        assert_eq!(response.body(), b"<h1>No such page</h1>");
        assert_eq!(response.content_type(), "text/html; charset=utf-8");
    }

    #[tokio::test]
    async fn an_unmatched_api_request_still_gets_the_json_envelope() {
        let response = router_with_a_not_found_page()
            .dispatch(request(Method::Get, "/nope"))
            .await;
        assert_eq!(response.status(), 404);
        assert!(
            response.body().starts_with(b"{"),
            "a caller that stated no Accept is programmatic and gets the envelope"
        );
    }

    #[tokio::test]
    async fn a_wrong_method_is_not_answered_by_the_not_found_page() {
        let response = router_with_a_not_found_page()
            .dispatch(navigating(Method::Delete, "/here"))
            .await;
        assert_eq!(response.status(), 405);
        assert!(response.body().starts_with(b"{"));
    }

    #[tokio::test]
    async fn a_router_without_a_page_answers_exactly_as_it_always_did() {
        let response = Router::<State>::new()
            .route(Route::html(
                "/here",
                |_ctx: HttpContext<State>| async move { Ok(String::new()) },
            ))
            .expect("unique route")
            .dispatch(navigation("/nope"))
            .await;
        assert_eq!(response.status(), 404);
        assert!(response.body().starts_with(b"{"));
    }

    #[tokio::test]
    async fn a_not_found_page_may_answer_with_its_own_status() {
        let response = Router::<State>::new()
            .not_found(Route::html(
                "__not_found",
                |ctx: HttpContext<State>| async move {
                    ctx.set_response_status(200);
                    Ok("<h1>Not open yet</h1>".to_owned())
                },
            ))
            .dispatch(navigation("/pt"))
            .await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), b"<h1>Not open yet</h1>");
    }

    #[tokio::test]
    async fn the_page_template_is_a_label_and_not_an_address() {
        let router = router_with_a_not_found_page();
        assert!(
            router.find(Method::Get, "/__not_found").is_err(),
            "the label must not be routable"
        );
        // Not counted among the routes, so `principal_gated_routes` cannot report it.
        assert_eq!(router.routes.len(), 1);
    }

    #[tokio::test]
    async fn router_middleware_wraps_the_not_found_page() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let router = Router::new()
            .middleware(Mark {
                label: "router",
                seen: seen.clone(),
            })
            .not_found(Route::html(
                "__not_found",
                |_ctx: HttpContext<State>| async move { Ok("<h1>gone</h1>".to_owned()) },
            ));

        let response = router.dispatch(navigation("/nowhere")).await;

        assert_eq!(response.status(), 404);
        assert_eq!(
            *seen.lock().expect("the recorder is not poisoned"),
            vec!["router"],
            "the page is counted, localized and gated like any other"
        );
    }

    /// A surface with two open markets, one closed one, and a Dutch slug.
    fn addressed_router() -> Router<State> {
        use fundamental_core::address::{
            AddressSpace, LocalePrefixSet, PageSlug, PrefixScope, SlugTable,
        };

        let prefixes = LocalePrefixSet::new()
            .with("nl", "nl-NL".parse().expect("tag"))
            .expect("nl")
            .with("nl-en", "en-NL".parse().expect("tag"))
            .expect("nl-en")
            .with_scope("pt", "en-PT".parse().expect("tag"), PrefixScope::RootOnly)
            .expect("pt");
        let slugs = SlugTable::builder()
            .page("/why", &[PageSlug::current("nl", "waarom")])
            .build()
            .expect("well formed");

        Router::<State>::new()
            .route(Route::html("/", |_ctx: HttpContext<State>| async move {
                Ok("<h1>home</h1>".to_owned())
            }))
            .expect("unique route")
            .route(Route::html("/why", |ctx: HttpContext<State>| async move {
                let prefix = ctx.address().map_or("", |address| address.prefix());
                Ok(format!("<h1>why at {prefix}</h1>"))
            }))
            .expect("unique route")
            // A non-page route: served wherever it was asked for.
            .route(Route::get("/robots.txt", |_ctx, _: ()| async move {
                Ok(Greeting {
                    text: "User-agent: *".to_owned(),
                })
            }))
            .expect("unique route")
            .with_addresses(std::sync::Arc::new(AddressSpace::new(prefixes, slugs)))
    }

    /// [`addressed_router`] plus the page that answers an address no route serves.
    fn addressed_router_with_a_not_found_page() -> Router<State> {
        addressed_router().not_found(Route::html(
            "__not_found",
            |ctx: HttpContext<State>| async move {
                let language = ctx
                    .locale()
                    .map_or("none", fundamental_core::i18n::Locale::language);
                let home = ctx
                    .address()
                    .map_or_else(|| "/".to_owned(), |address| address.href("/"));
                Ok(format!("<p>{language}</p><a href=\"{home}\">home</a>"))
            },
        ))
    }

    fn english_rooted_router() -> Router<State> {
        use fundamental_core::address::{
            AddressSpace, LocalePrefixSet, PageSlug, PrefixScope, SlugTable,
        };

        let prefixes = LocalePrefixSet::new()
            .with_paths_in("nl", "nl-NL".parse().expect("tag"), PrefixScope::Full, "en")
            .expect("nl");
        let slugs = SlugTable::builder()
            .page("/documenten", &[PageSlug::current("en", "documents")])
            .page("/{team}", &[])
            .page(
                "/{team}/aanvragen",
                &[PageSlug::current("en", "build-requests")],
            )
            .build()
            .expect("well formed");
        let space = AddressSpace::new(prefixes, slugs)
            .with_default_language("en".parse().expect("tag"))
            .expect("en is not prefixed");

        Router::<State>::new()
            .route(Route::html(
                "/documenten",
                |_ctx: HttpContext<State>| async move { Ok("<h1>documents</h1>".to_owned()) },
            ))
            .expect("unique route")
            .route(Route::html(
                "/{team}/aanvragen",
                |_ctx: HttpContext<State>| async move { Ok("<h1>build requests</h1>".to_owned()) },
            ))
            .expect("unique route")
            .document(
                "/documenten/voorwaarden.pdf",
                StaticAsset::typed(b"%PDF-1.7".to_vec(), "application/pdf"),
            )
            .expect("a free path")
            .with_addresses(Arc::new(space))
    }

    #[tokio::test]
    async fn the_canonical_spelling_redirect_never_leaves_its_origin() {
        let router = english_rooted_router();
        for asked in ["/\\evil.example/aanvragen", "//evil.example/aanvragen"] {
            let response = router.dispatch(navigation(asked)).await;
            let location = response
                .headers()
                .iter()
                .find(|(name, _)| name == "location")
                .map(|(_, value)| value.clone());
            assert!(
                location.as_deref().is_none_or(
                    |location| !location.starts_with("//") && !location.starts_with("/\\")
                ),
                "{asked} answered {} at {location:?}",
                response.status()
            );
        }
        let spelled = router.dispatch(navigation("/acme/aanvragen")).await;
        assert_eq!(
            spelled.status(),
            301,
            "an ordinary Dutch spelling still answers the canonical-spelling redirect"
        );
    }

    #[tokio::test]
    async fn a_document_is_served_at_every_spelling_of_its_directory() {
        let router = english_rooted_router();
        for asked in [
            "/documents/voorwaarden.pdf",
            "/nl/documents/voorwaarden.pdf",
            "/documenten/voorwaarden.pdf",
        ] {
            let response = router.dispatch(navigation(asked)).await;
            assert_eq!(response.status(), 200, "{asked}");
            assert_eq!(response.content_type(), "application/pdf", "{asked}");
            assert_eq!(response.body(), b"%PDF-1.7", "{asked}");
        }
    }

    #[tokio::test]
    async fn a_missing_page_inside_a_market_is_answered_in_that_markets_language() {
        let request = navigation("/nl/onzin").with_locale(Some("de".parse().expect("tag")));
        let response = addressed_router_with_a_not_found_page()
            .dispatch(request)
            .await;

        assert_eq!(response.status(), 404);
        assert!(
            String::from_utf8_lossy(response.body()).contains("<p>nl</p>"),
            "the market decided the language for every other page at this prefix"
        );
        assert!(
            response.language_invariant(),
            "a 404 whose language came from the URL varies on nothing about the reader"
        );
    }

    #[tokio::test]
    async fn a_missing_page_inside_a_market_can_build_links_back_into_it() {
        let response = addressed_router_with_a_not_found_page()
            .dispatch(navigation("/nl/onzin"))
            .await;

        assert!(
            String::from_utf8_lossy(response.body()).contains("href=\"/nl\""),
            "an unprefixed link would send the one reader who is already lost \
             out of the market they were in"
        );
    }

    /// The other half: no prefix, no address — the page must not invent one.
    #[tokio::test]
    async fn a_missing_page_outside_a_market_carries_no_address() {
        let response = addressed_router_with_a_not_found_page()
            .dispatch(navigation("/onzin"))
            .await;

        assert_eq!(response.status(), 404);
        assert!(
            String::from_utf8_lossy(response.body()).contains("href=\"/\""),
            "with no market to be in, home is the site root"
        );
        assert!(
            !response.language_invariant(),
            "nothing about the URL decided the language, so the cache must still \
             hear what the negotiation said"
        );
    }

    #[tokio::test]
    async fn a_localized_address_reaches_the_route_registered_at_its_canonical_path() {
        let response = addressed_router().dispatch(navigation("/nl/waarom")).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), b"<h1>why at /nl</h1>");
    }

    #[tokio::test]
    async fn the_address_decides_the_language_whatever_the_header_said() {
        let request = navigation("/nl/waarom").with_locale(Some("de".parse().expect("tag")));
        let response = addressed_router().dispatch(request).await;
        assert_eq!(response.status(), 200);
        assert!(
            response.language_invariant(),
            "the response must say the URL is its whole cache key"
        );
    }

    #[tokio::test]
    async fn an_untranslated_stem_redirects_to_the_one_address() {
        let response = addressed_router().dispatch(navigation("/nl/why")).await;
        assert_eq!(response.status(), 301);
        assert_eq!(location(&response), Some("/nl/waarom"));
    }

    #[tokio::test]
    async fn a_path_with_no_market_is_sent_home_rather_than_into_one() {
        let response = addressed_router().dispatch(navigation("/why")).await;
        assert_eq!(response.status(), 301);
        assert_eq!(
            location(&response),
            Some("/"),
            "resolving it to a market would be a guess about the reader"
        );
    }

    #[tokio::test]
    async fn an_address_correction_carries_the_query_the_reader_sent() {
        let response = addressed_router()
            .dispatch(navigation("/why").with_query("tab=files"))
            .await;
        assert_eq!(response.status(), 301);
        assert_eq!(
            location(&response),
            Some("/?tab=files"),
            "the address was corrected and the reader's own question was dropped"
        );
    }

    /// The same, on a correction that changes the path rather than only the prefix.
    #[tokio::test]
    async fn an_untranslated_stem_keeps_the_query_across_the_translation() {
        let response = addressed_router()
            .dispatch(navigation("/nl/why").with_query("tab=files&page=2"))
            .await;
        assert_eq!(response.status(), 301);
        assert_eq!(location(&response), Some("/nl/waarom?tab=files&page=2"));
    }

    /// What `dispatch` writes to a formatter at every level, spans included.
    async fn logs_of(router: &Router<State>, request: ServerRequest<State>) -> (u16, String) {
        use tracing::instrument::WithSubscriber;
        use tracing_subscriber::fmt::format::FmtSpan;

        #[derive(Clone, Default)]
        struct Buffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Buffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0
                    .lock()
                    .map_err(|_| std::io::Error::other("poisoned"))?
                    .extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
            .with_writer(move || writer.clone())
            .finish();
        let status = router
            .dispatch(request)
            .with_subscriber(subscriber)
            .await
            .status();
        let bytes = buffer.0.lock().expect("not poisoned").clone();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn a_canonical_redirect_logs_the_route_and_never_the_query() {
        let router = addressed_router();
        let (status, logs) = logs_of(
            &router,
            navigation("/nl/why").with_query("email=jan@example.nl"),
        )
        .await;
        assert_eq!(status, 301);
        assert!(
            logs.contains("redirected to the canonical address"),
            "{logs}"
        );
        assert!(logs.contains("query_bytes=20"), "{logs}");
        assert!(!logs.contains("jan"), "{logs}");

        let padded = format!("email=jan@example.nl&pad={}", "p".repeat(4096));
        let (status, logs) = logs_of(&router, navigation("/nl/why").with_query(padded)).await;
        assert_eq!(status, 200, "a refused target serves in place");
        assert!(logs.contains("not a usable redirect target"), "{logs}");
        assert!(!logs.contains("jan"), "{logs}");
        assert!(!logs.contains(&"p".repeat(64)), "{logs}");
    }

    #[tokio::test]
    async fn an_address_correction_without_a_query_gains_no_question_mark() {
        let response = addressed_router().dispatch(navigation("/nl/why")).await;
        assert_eq!(response.status(), 301);
        assert_eq!(location(&response), Some("/nl/waarom"));
    }

    #[tokio::test]
    async fn a_prefix_that_serves_one_page_folds_every_deeper_path_into_it() {
        let response = addressed_router().dispatch(navigation("/pt/waarom")).await;
        assert_eq!(response.status(), 301);
        assert_eq!(location(&response), Some("/pt"));

        let root = addressed_router().dispatch(navigation("/pt")).await;
        assert_eq!(root.status(), 200, "the closed market's own page serves");
    }

    /// The gate that stops the wasm bundle being redirected out of existence.
    #[tokio::test]
    async fn a_non_page_route_is_served_where_it_was_asked_for() {
        let response = addressed_router()
            .dispatch(request(Method::Get, "/robots.txt"))
            .await;
        assert_eq!(
            response.status(),
            200,
            "a non-html route has no localized address to be redirected to"
        );
        assert!(location(&response).is_none());
    }

    #[tokio::test]
    async fn a_non_get_is_never_redirected_to_an_address() {
        let router = addressed_router()
            .route(Route::post(
                "/why",
                |_ctx: HttpContext<State>, _: ()| async move {
                    Ok(Greeting {
                        text: "posted".to_owned(),
                    })
                },
            ))
            .expect("unique route");
        let response = router
            .dispatch(navigating(Method::Post, "/why").with_body(b"{}".to_vec()))
            .await;
        assert_ne!(response.status(), 301);
    }

    #[tokio::test]
    async fn the_route_template_stays_canonical_so_a_metric_is_not_per_market() {
        // If the template followed the address, a dashboard would grow a series per market.
        for address in ["/nl/waarom", "/nl-en/why"] {
            let response = addressed_router().dispatch(navigation(address)).await;
            assert_eq!(response.status(), 200, "{address}");
        }
    }

    #[test]
    fn the_route_table_does_not_grow_with_the_locale_set() {
        assert_eq!(
            addressed_router().routes.len(),
            3,
            "three routes serve three markets"
        );
    }

    #[tokio::test]
    async fn a_router_with_no_address_space_behaves_exactly_as_before() {
        let response = router_with_a_not_found_page()
            .dispatch(navigation("/here"))
            .await;
        assert_eq!(response.status(), 200);
        assert!(!response.language_invariant());
        assert!(location(&response).is_none());
    }

    // Marking it preference-invariant would drop `Vary: Cookie` from every addressed page.
    #[tokio::test]
    async fn an_addressed_page_is_language_invariant_and_not_preference_invariant() {
        let response = addressed_router().dispatch(navigation("/nl/waarom")).await;

        assert!(
            response.language_invariant(),
            "the address is the whole cache key for language"
        );
        assert!(
            !response.preference_invariant(),
            "an addressed page still renders differently for a reader who pinned one"
        );
    }

    #[tokio::test]
    async fn a_static_asset_does_not_vary_on_the_readers_language() {
        let router = Router::<State>::new()
            .static_asset(
                "/app.wasm",
                StaticAsset::new(Path::new("app.wasm"), b"\0asm".to_vec()),
            )
            .route(Route::html("/", |_ctx: HttpContext<State>| async move {
                Ok(String::new())
            }))
            .expect("unique route");

        let asset = router.dispatch(navigation("/app.wasm")).await;
        assert_eq!(asset.status(), 200);
        assert!(
            asset.language_invariant(),
            "the bundle has one representation, not one per language"
        );
        assert!(
            asset.preference_invariant(),
            "…nor one per display preference: a `Cookie` vary on the bundle fragments \
             the CDN once per reader, for bytes that never differ"
        );

        let page = router.dispatch(navigation("/")).await;
        assert!(!page.language_invariant());
        assert!(!page.preference_invariant());
    }

    fn location(response: &ServerResponse) -> Option<&str> {
        response
            .headers()
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("location"))
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn an_accept_header_is_read_as_a_question_about_html() {
        assert!(accepts_html(Some(
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
        )));
        assert!(accepts_html(Some("*/*")), "curl states no preference");
        assert!(
            accepts_html(Some("TEXT/HTML")),
            "media types are ASCII-insensitive"
        );
        assert!(accepts_html(Some("text/*")));
        assert!(!accepts_html(Some("application/json")));
        assert!(
            !accepts_html(None),
            "no Accept at all is a programmatic caller, and markup is the wrong answer for it"
        );
    }

    /// A refusal that knows its delay and keeps a secret in its developer message.
    #[derive(Debug)]
    struct Overloaded;

    impl core::fmt::Display for Overloaded {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("overloaded")
        }
    }

    impl std::error::Error for Overloaded {}

    impl Error for Overloaded {
        fn error_code(&self) -> &'static str {
            "TEST_503"
        }

        fn display_message(&self) -> Cow<'_, str> {
            Cow::Borrowed("Busy right now.")
        }

        fn developer_message(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed("pool db-7 exhausted"))
        }

        fn http_status_code(&self) -> u16 {
            503
        }

        fn retry_after_secs(&self) -> Option<u64> {
            Some(17)
        }
    }

    fn router_with_an_error_page(
        seen: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
    ) -> Router<State> {
        Router::<State>::new()
            .route(Route::html(
                "/busy",
                |_ctx: HttpContext<State>| async move {
                    Err(fundamental_core::AnyError::new(Overloaded))
                },
            ))
            .expect("unique route")
            .route(Route::html(
                "/here",
                |_ctx: HttpContext<State>| async move { Ok("<p>here</p>".to_owned()) },
            ))
            .expect("unique route")
            .middleware(Mark {
                label: "router layer",
                seen,
            })
            .error_page(Route::html(
                "__error",
                |ctx: HttpContext<State>| async move {
                    let shown = ctx.shown_error().expect("the page is told what it shows");
                    Ok(format!(
                        "<h1>{} {} {:?} q={} h={}</h1>",
                        shown.code(),
                        shown.status(),
                        shown.retry_after_secs(),
                        ctx.query_raw(),
                        ctx.headers().get("accept").unwrap_or("none"),
                    ))
                },
            ))
    }

    fn seen() -> std::sync::Arc<std::sync::Mutex<Vec<&'static str>>> {
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))
    }

    fn fetch_metadata(path: &str, mode: &str) -> ServerRequest<State> {
        request(Method::Get, path).with_headers(
            [("sec-fetch-mode".to_owned(), mode.to_owned())]
                .into_iter()
                .collect(),
        )
    }

    fn header<'a>(response: &'a ServerResponse, name: &str) -> Vec<&'a str> {
        response
            .headers()
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
            .collect()
    }

    #[tokio::test]
    async fn a_handler_error_renders_the_page_for_a_navigation() {
        let response = router_with_an_error_page(seen())
            .dispatch(fetch_metadata("/busy", "navigate").with_query("secret=1"))
            .await;
        assert_eq!(
            response.status(),
            503,
            "the error's status, not the page's 200"
        );
        assert_eq!(response.content_type(), "text/html; charset=utf-8");
        let body = String::from_utf8_lossy(response.body()).into_owned();
        assert!(body.starts_with("<h1>TEST_503 503 Some(17)"), "{body}");
        assert_eq!(header(&response, "retry-after"), ["17"]);
        assert_eq!(header(&response, "cache-control"), ["no-store"]);
        assert_eq!(header(&response, "x-robots-tag"), ["noindex"]);
        assert_eq!(header(&response, "vary"), ["Sec-Fetch-Mode, Accept"]);
        assert_eq!(response.shown_error(), None, "answered once, never again");
    }

    fn router_with_refusals(gate: bool) -> Router<State> {
        let router = Router::<State>::new()
            .route(Route::html("/mine", |ctx: HttpContext<State>| async move {
                ctx.set_response_header("Cache-Control", "public, max-age=600");
                Err::<String, _>(fundamental_core::AnyError::new(
                    ServerError::unauthenticated(),
                ))
            }))
            .expect("unique route")
            .route(Route::html(
                "/theirs",
                |_ctx: HttpContext<State>| async move {
                    Err::<String, _>(fundamental_core::AnyError::new(Refused403))
                },
            ))
            .expect("unique route")
            .error_page(Route::html(
                "__error",
                |ctx: HttpContext<State>| async move {
                    let shown = ctx.shown_error().expect("the page is told what it shows");
                    Ok(format!("<h1>error {}</h1>", shown.status()))
                },
            ));
        if !gate {
            return router;
        }
        router.gate_page(Route::html(
            "__gate",
            |ctx: HttpContext<State>| async move {
                let shown = ctx.shown_error().expect("the gate is told what it answers");
                Ok(format!("<h1>gate {} {}</h1>", shown.status(), shown.code()))
            },
        ))
    }

    #[derive(Debug)]
    struct Refused403;

    impl core::fmt::Display for Refused403 {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("refused")
        }
    }

    impl std::error::Error for Refused403 {}

    impl Error for Refused403 {
        fn error_code(&self) -> &'static str {
            "TEST_403"
        }

        fn display_message(&self) -> Cow<'_, str> {
            Cow::Borrowed("Not yours.")
        }

        fn http_status_code(&self) -> u16 {
            403
        }
    }

    #[tokio::test]
    async fn a_refusal_on_a_navigation_renders_the_gate() {
        let router = router_with_refusals(true);
        let signed_out = router.dispatch(fetch_metadata("/mine", "navigate")).await;
        assert_eq!(signed_out.status(), 401);
        let body = String::from_utf8_lossy(signed_out.body()).into_owned();
        assert!(body.starts_with("<h1>gate 401 "), "{body}");
        assert_eq!(
            header(&signed_out, "cache-control"),
            ["no-store"],
            "one Cache-Control, and the handler's public one is gone"
        );
        assert_eq!(header(&signed_out, "x-robots-tag"), ["noindex"]);

        let forbidden = router.dispatch(fetch_metadata("/theirs", "navigate")).await;
        assert_eq!(forbidden.status(), 403);
        let body = String::from_utf8_lossy(forbidden.body()).into_owned();
        assert_eq!(body, "<h1>gate 403 TEST_403</h1>");
        assert_eq!(header(&forbidden, "cache-control"), ["no-store"]);

        let program = router.dispatch(fetch_metadata("/theirs", "cors")).await;
        assert!(
            program.body().starts_with(b"{"),
            "a program keeps the envelope"
        );
    }

    #[tokio::test]
    async fn a_refusal_without_a_gate_keeps_the_error_page() {
        let response = router_with_refusals(false)
            .dispatch(fetch_metadata("/theirs", "navigate"))
            .await;
        assert_eq!(response.status(), 403);
        assert_eq!(response.body(), b"<h1>error 403</h1>");
        assert_eq!(header(&response, "cache-control"), ["no-store"]);
    }

    #[tokio::test]
    async fn the_page_never_carries_the_developer_message_or_query() {
        let response = router_with_an_error_page(seen())
            .dispatch(fetch_metadata("/busy", "navigate").with_query("secret=1"))
            .await;
        let body = String::from_utf8_lossy(response.body()).into_owned();
        assert!(!body.contains("db-7"), "{body}");
        assert!(
            !body.contains("Busy right now"),
            "the page picks its own words: {body}"
        );
        assert!(
            body.contains("q= h=none"),
            "no query and no header reach the page: {body}"
        );
    }

    #[tokio::test]
    async fn a_fetch_keeps_the_envelope_and_learns_it_varies() {
        let response = router_with_an_error_page(seen())
            .dispatch(fetch_metadata("/busy", "cors"))
            .await;
        assert_eq!(response.status(), 503);
        assert!(
            response.body().starts_with(b"{"),
            "a program reads the envelope"
        );
        assert!(!String::from_utf8_lossy(response.body()).contains("db-7"));
        assert_eq!(header(&response, "retry-after"), ["17"]);
        assert_eq!(header(&response, "vary"), ["Sec-Fetch-Mode, Accept"]);
    }

    #[tokio::test]
    async fn without_fetch_metadata_html_by_name_is_a_navigation_and_any_is_not() {
        let router = router_with_an_error_page(seen());
        let named = router.dispatch(navigation("/busy")).await;
        assert!(
            named.body().starts_with(b"<h1>TEST_503"),
            "Accept names text/html"
        );
        let any = router
            .dispatch(
                request(Method::Get, "/busy").with_headers(
                    [("accept".to_owned(), "*/*".to_owned())]
                        .into_iter()
                        .collect(),
                ),
            )
            .await;
        assert!(
            any.body().starts_with(b"{"),
            "curl's */* keeps the envelope"
        );
    }

    #[tokio::test]
    async fn the_error_page_skips_router_middleware() {
        let seen = seen();
        let response = router_with_an_error_page(seen.clone())
            .dispatch(fetch_metadata("/busy", "navigate"))
            .await;
        assert!(response.body().starts_with(b"<h1>"));
        assert_eq!(
            *seen.lock().expect("the recorder is not poisoned"),
            ["router layer"],
            "the layer ran once, around the failing route, and not around the page"
        );
    }

    #[tokio::test]
    async fn a_failing_error_page_keeps_the_envelope() {
        let router = Router::<State>::new()
            .route(Route::html(
                "/busy",
                |_ctx: HttpContext<State>| async move {
                    Err(fundamental_core::AnyError::new(Overloaded))
                },
            ))
            .expect("unique route")
            .error_page(Route::html(
                "__error",
                |_ctx: HttpContext<State>| async move {
                    Err(fundamental_core::AnyError::new(ServerError::internal(
                        "page broke",
                    )))
                },
            ));
        let response = router.dispatch(fetch_metadata("/busy", "navigate")).await;
        assert_eq!(response.status(), 503);
        assert!(response.body().starts_with(b"{\"error\":\"TEST_503\""));
        assert_eq!(response.shown_error(), None, "the page is not tried again");
    }

    #[tokio::test]
    async fn a_routes_404_is_answered_with_the_not_found_page_where_there_is_one() {
        let router = router_with_an_error_page(seen())
            .route(Route::html(
                "/gone",
                |_ctx: HttpContext<State>| async move {
                    Err(fundamental_core::AnyError::new(ServerError::NotFound {
                        method: "GET".into(),
                        path: crate::PathShape::new("/gone", |_| true),
                    }))
                },
            ))
            .expect("unique route")
            .not_found(Route::html(
                "__not_found",
                |_ctx: HttpContext<State>| async move { Ok("<h1>No such page</h1>".to_owned()) },
            ));
        let response = router.dispatch(fetch_metadata("/gone", "navigate")).await;
        assert_eq!(response.status(), 404);
        assert_eq!(response.body(), b"<h1>No such page</h1>");
        let other = router.dispatch(fetch_metadata("/busy", "navigate")).await;
        assert!(
            other.body().starts_with(b"<h1>TEST_503"),
            "the rest keep the error page"
        );
    }

    #[tokio::test]
    async fn a_page_that_renders_is_never_replaced() {
        let response = router_with_an_error_page(seen())
            .dispatch(fetch_metadata("/here", "navigate"))
            .await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), b"<p>here</p>");
        assert!(header(&response, "vary").is_empty());
    }

    #[tokio::test]
    async fn a_refusal_before_the_router_is_answered_with_the_page_too() {
        let router = router_with_an_error_page(seen());
        let navigation = fetch_metadata("/here", "navigate");
        let page_request = router.error_page_request(&navigation);
        assert!(page_request.is_some(), "a navigation gets a page request");
        let response = router
            .answer_navigation(ServerResponse::from_error(&Overloaded), page_request)
            .await;
        assert!(response.body().starts_with(b"<h1>TEST_503"));
        assert!(
            router
                .error_page_request(&fetch_metadata("/here", "cors"))
                .is_none(),
            "a program's request never pays for one"
        );
    }

    /// What a whole site hands its pages, such as the document shell they wear.
    #[derive(Debug, PartialEq)]
    struct Shell(&'static str);

    #[tokio::test]
    async fn a_handler_reads_the_extension_its_router_holds() {
        let router = Router::<State>::new()
            .route(Route::html("/", |ctx: HttpContext<State>| async move {
                Ok(ctx
                    .extension::<Shell>()
                    .map_or("none", |shell| shell.0)
                    .to_owned())
            }))
            .expect("unique route")
            .with_extension(Shell("brand"));
        let response = router.dispatch(request(Method::Get, "/")).await;
        assert_eq!(response.body(), b"brand");
    }

    #[tokio::test]
    async fn a_router_without_the_extension_hands_none() {
        let router = Router::<State>::new()
            .route(Route::html("/", |ctx: HttpContext<State>| async move {
                Ok(ctx
                    .extension::<Shell>()
                    .map_or("none", |shell| shell.0)
                    .to_owned())
            }))
            .expect("unique route");
        let response = router.dispatch(request(Method::Get, "/")).await;
        assert_eq!(response.body(), b"none");
    }

    #[test]
    fn the_router_lists_the_pages_it_serves_at_a_fixed_path() {
        let page = |path: &str| {
            Route::html(
                path,
                |_ctx: HttpContext<State>| async move { Ok(String::new()) },
            )
        };
        let router = Router::<State>::new()
            .route(page("/mission"))
            .expect("unique route")
            .route(page("/"))
            .expect("unique route")
            .route(page("/projects/{slug}"))
            .expect("unique route")
            .route(Route::get(
                "/api/me",
                |_ctx: HttpContext<State>, _: ()| async move {
                    Ok(Greeting {
                        text: String::new(),
                    })
                },
            ))
            .expect("unique route");
        assert_eq!(router.page_paths(), ["/", "/mission"]);
    }
}
