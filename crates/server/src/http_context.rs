use crate::Peer;
use fundamental_auth::AuthenticatedPrincipal;
use fundamental_core::Error;
use fundamental_core::address::{AddressSpace, PagePath, RequestAddress};
use fundamental_core::asset::AssetAddresses;
use fundamental_core::error::ShownError;
use fundamental_core::i18n::Locale;
use serde::de::DeserializeOwned;
use std::sync::Arc;

use crate::ActionDispatcher;
use crate::action_dispatcher::ActionRoutes;
use crate::cookie_lookup::CookieLookup;
use crate::from_ref::FromRef;
use crate::request_body_slot::RequestBodySlot;
use crate::response_stream_slot::ResponseStreamSlot;
use crate::{
    PathParams, Redirect, RequestHeaders, ResponseHeaders, ResponseRedirect, ResponseStatus,
    ServerError, ServerResult,
};

/// The per-request context handed to a handler, generic over the application's typed state `S`.
#[derive(Debug, Clone)]
pub struct HttpContext<S> {
    state: S,
    request_id: String,
    query: String,
    params: PathParams,
    principal: Option<AuthenticatedPrincipal>,
    locale: Option<Locale>,
    preferences: Vec<(&'static str, &'static str)>,
    headers: RequestHeaders,
    response_headers: ResponseHeaders,
    response_status: ResponseStatus,
    response_redirect: ResponseRedirect,
    route_template: Option<std::sync::Arc<str>>,
    method: Option<crate::Method>,
    address: Option<RequestAddress>,
    /// The version of every asset the router holds, for the links a page writes.
    asset_addresses: Arc<AssetAddresses>,
    /// The socket this request arrived on: the only thing in a request the caller did not write.
    peer: Option<Peer>,
    /// The streamed request body, taken once by the handler.
    body_stream: RequestBodySlot,
    /// Where a streaming handler leaves its response body.
    response_stream: ResponseStreamSlot,
    /// What the request's authorizing middleware allowed.
    authorized: crate::AuthorizedObjects,
    segments: crate::SegmentLedger,
    /// The error the router's error page is answering, on that page's request only.
    shown_error: Option<ShownError>,
    action_routes: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>,
    /// What the router hands every handler, one value per type.
    extensions: crate::extensions::Extensions,
}

impl<S> HttpContext<S> {
    /// Creates a context over `state` for the request identified by `request_id`.
    pub fn new(state: S, request_id: impl Into<String>) -> Self {
        Self {
            state,
            request_id: request_id.into(),
            query: String::new(),
            params: PathParams::new(),
            principal: None,
            locale: None,
            preferences: Vec::new(),
            headers: RequestHeaders::new(),
            response_headers: ResponseHeaders::new(),
            response_status: ResponseStatus::new(),
            response_redirect: ResponseRedirect::new(),
            address: None,
            asset_addresses: Arc::default(),
            route_template: None,
            method: None,
            peer: None,
            body_stream: RequestBodySlot::default(),
            response_stream: ResponseStreamSlot::default(),
            authorized: crate::AuthorizedObjects::default(),
            segments: crate::SegmentLedger::default(),
            shown_error: None,
            action_routes: None,
            extensions: crate::extensions::Extensions::default(),
        }
    }

    /// Attaches the error the router's error page is answering.
    #[must_use]
    pub fn with_shown_error(mut self, shown_error: Option<ShownError>) -> Self {
        self.shown_error = shown_error;
        self
    }

    /// The error this request's page is answering; set on the [`error_page`](crate::Router::error_page) only.
    #[must_use]
    pub fn shown_error(&self) -> Option<ShownError> {
        self.shown_error
    }

    /// The request-scoped dispatcher for actions mounted on this router, when present.
    pub fn action_dispatcher(&self) -> Option<ActionDispatcher>
    where
        S: Clone + Send + Sync + 'static,
    {
        self.action_routes
            .as_ref()
            .and_then(|routes| {
                std::sync::Arc::clone(routes)
                    .downcast::<ActionRoutes<S>>()
                    .ok()
            })
            .map(|routes| routes.for_request(self.clone()))
    }

    pub(crate) fn with_action_routes(
        mut self,
        routes: std::sync::Arc<dyn std::any::Any + Send + Sync>,
    ) -> Self {
        self.action_routes = Some(routes);
        self
    }

    /// Gives an in-process action call its own route metadata, captures, query and sinks, keeping the request's identity.
    pub(crate) fn for_internal_action(
        mut self,
        method: crate::Method,
        template: std::sync::Arc<str>,
        params: PathParams,
        query: &str,
    ) -> Self {
        query.clone_into(&mut self.query);
        self.params = params;
        self.method = Some(method);
        self.route_template = Some(template);
        self.response_headers = ResponseHeaders::new();
        self.response_status = ResponseStatus::new();
        self.response_redirect = ResponseRedirect::new();
        self.body_stream = RequestBodySlot::default();
        self.response_stream = ResponseStreamSlot::default();
        self.authorized = crate::AuthorizedObjects::default();
        self.segments = crate::SegmentLedger::default();
        self.shown_error = None;
        self
    }

    /// Attaches the template of the route that matched.
    #[must_use]
    pub fn with_route_template(mut self, template: std::sync::Arc<str>) -> Self {
        self.route_template = Some(template);
        self
    }

    /// Hands this request's streamed body to the context, for the handler to take.
    #[must_use]
    pub(crate) fn with_body_stream(mut self, body: Option<crate::RequestBodyStream>) -> Self {
        if let Some(body) = body {
            self.body_stream = RequestBodySlot::holding(body);
        }
        self
    }

    /// Takes the request's streamed body; only the first call answers `Some`.
    #[must_use]
    pub fn take_body_stream(&self) -> Option<crate::RequestBodyStream> {
        self.body_stream.take()
    }

    /// What this request's authorizing middleware recorded it allowed.
    #[must_use]
    pub fn authorized_objects(&self) -> &crate::AuthorizedObjects {
        &self.authorized
    }

    /// What this request's declared path segments resolved to.
    pub fn segments(&self) -> &crate::SegmentLedger {
        &self.segments
    }

    /// Records that the reader may see the record `param` names, for a loader that checked access itself.
    pub fn affirm(&self, param: &str) {
        self.segments.affirm(param);
    }

    pub(crate) fn response_stream_slot(&self) -> ResponseStreamSlot {
        self.response_stream.clone()
    }

    /// Attaches the request's method.
    #[must_use]
    pub fn with_method(mut self, method: crate::Method) -> Self {
        self.method = Some(method);
        self
    }

    /// The request's method.
    #[must_use]
    pub fn method(&self) -> Option<crate::Method> {
        self.method
    }

    /// The template of the route that matched, `"/users/{id}"`: the label a metric is grouped by.
    #[must_use]
    pub fn route_template(&self) -> Option<&str> {
        self.route_template.as_deref()
    }

    /// Attaches the path parameters captured from the matched route.
    #[must_use]
    pub fn with_params(mut self, params: PathParams) -> Self {
        self.params = params;
        self
    }

    /// Attaches the raw URL query string (everything after `?`).
    #[must_use]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self
    }

    /// Attaches an authenticated principal, if one was resolved.
    #[must_use]
    pub fn with_principal(mut self, principal: Option<AuthenticatedPrincipal>) -> Self {
        self.principal = principal;
        self
    }

    /// Attaches the language this request is answered in.
    #[must_use]
    pub fn with_locale(mut self, locale: Option<Locale>) -> Self {
        self.locale = locale;
        self
    }

    /// Attaches the display preferences this request pinned.
    #[must_use]
    pub fn with_preferences(mut self, preferences: Vec<(&'static str, &'static str)>) -> Self {
        self.preferences = preferences;
        self
    }

    /// The value this request pinned for `name`, if it pinned one this build offers; never a cookie.
    #[must_use]
    pub fn preference(&self, name: &str) -> Option<&'static str> {
        self.preferences
            .iter()
            .find(|(pinned, _)| *pinned == name)
            .map(|(_, value)| *value)
    }

    /// Every preference this request pinned, for handing on to a render.
    #[must_use]
    pub fn preferences(&self) -> &[(&'static str, &'static str)] {
        &self.preferences
    }

    /// Attaches the address this page is being rendered at.
    #[must_use]
    pub fn with_address(mut self, address: Option<RequestAddress>) -> Self {
        self.address = address;
        self
    }

    /// Attaches `value`, read back by its type through [`extension`](Self::extension).
    #[must_use]
    pub fn with_extension<T: std::any::Any + Send + Sync>(mut self, value: T) -> Self {
        self.extensions = self.extensions.with(value);
        self
    }

    /// The value of type `T` the router hands every handler, if it holds one.
    #[must_use]
    pub fn extension<T: std::any::Any + Send + Sync>(&self) -> Option<&T> {
        self.extensions.get::<T>()
    }

    /// Whether the router serves the development live reload.
    #[must_use]
    pub fn serves_live_reload(&self) -> bool {
        self.extension::<crate::live_reload::LiveReloadMounted>()
            .is_some()
    }

    pub(crate) fn with_extensions(mut self, extensions: crate::extensions::Extensions) -> Self {
        self.extensions = extensions;
        self
    }

    /// Attaches the versions of the router's assets.
    #[must_use]
    pub fn with_asset_addresses(mut self, asset_addresses: Arc<AssetAddresses>) -> Self {
        self.asset_addresses = asset_addresses;
        self
    }

    /// The version of every asset the router holds, by path.
    #[must_use]
    pub fn asset_addresses(&self) -> &Arc<AssetAddresses> {
        &self.asset_addresses
    }

    /// Attaches the request headers captured at the boundary.
    #[must_use]
    pub fn with_headers(mut self, headers: RequestHeaders) -> Self {
        self.headers = headers;
        self
    }

    /// Records the socket this request arrived on; no header or body can set it.
    #[must_use]
    pub fn with_peer(mut self, peer: Option<Peer>) -> Self {
        self.peer = peer;
        self
    }

    /// Returns the application's typed state.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Returns the request's correlation id, for log/trace stitching.
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Returns the path parameters captured from the matched route.
    #[must_use]
    pub fn params(&self) -> &PathParams {
        &self.params
    }

    /// Returns the language this request is answered in, if the surface negotiates.
    #[must_use]
    pub fn locale(&self) -> Option<&Locale> {
        self.locale.as_ref()
    }

    /// Returns the address this page is being rendered at, when the surface has an address space.
    #[must_use]
    pub fn address(&self) -> Option<&RequestAddress> {
        self.address.as_ref()
    }

    /// The page a route builder named, spelled as this request's own page spells it.
    ///
    /// ```ignore
    /// ctx.redirect(Redirect::see_other(ctx.href_to(&routes::sign_in_code()))?);
    /// ```
    #[must_use]
    pub fn href_to(&self, page: &PagePath) -> String {
        self.address.as_ref().map_or_else(
            || AddressSpace::default().page_address("", page),
            |address| address.href_to(page),
        )
    }

    /// Returns the value of the request header named `name`, matched case-insensitively.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    /// Returns all request headers captured at the boundary.
    #[must_use]
    pub fn headers(&self) -> &RequestHeaders {
        &self.headers
    }

    /// The socket this request arrived on; `None` for the native in-process bridge.
    #[must_use]
    pub fn peer(&self) -> Option<Peer> {
        self.peer
    }

    /// Whether this request is a browser's own `<form>` submission, by the same predicate the decoder uses.
    #[must_use]
    pub fn is_form_submission(&self) -> bool {
        let content_type = self.header("content-type");
        crate::http_handler::is_form(content_type)
            || crate::http_handler::is_multipart(content_type)
    }

    /// Returns the value of the request cookie named `name`, if the request carried exactly one.
    ///
    /// Reads every `Cookie` field, case-sensitively; a name sent with two values reads as [`None`].
    #[must_use]
    pub fn cookie(&self, name: &str) -> Option<&str> {
        match CookieLookup::find(self.headers.get_all("cookie"), name) {
            CookieLookup::Found(value) => Some(value),
            CookieLookup::Absent | CookieLookup::Ambiguous(_) => None,
        }
    }

    /// Records a response header for this request's reply, on success and error alike.
    pub fn set_response_header(&self, name: impl Into<String>, value: impl Into<String>) {
        self.response_headers.set(name, value);
    }

    /// Returns a clone of the response-header sink handle.
    #[must_use]
    pub fn response_headers(&self) -> ResponseHeaders {
        self.response_headers.clone()
    }

    /// Sets the success HTTP status of this request's reply; an error's own status still wins.
    pub fn set_response_status(&self, status: u16) {
        self.response_status.set(status);
    }

    /// Returns a clone of the response-status sink handle.
    #[must_use]
    pub fn response_status(&self) -> ResponseStatus {
        self.response_status.clone()
    }

    /// Makes this request's reply a [`Redirect`], on the error path as well as the success path.
    ///
    /// The error is still logged; the status is mirrored into the status sink for observers.
    pub fn redirect(&self, redirect: Redirect) {
        self.response_status.set(redirect.status());
        self.response_redirect.set(redirect);
    }

    /// Returns a clone of the redirect sink handle.
    #[must_use]
    pub fn response_redirect(&self) -> ResponseRedirect {
        self.response_redirect.clone()
    }

    /// Returns the raw URL query string (everything after `?`, without the `?`).
    #[must_use]
    pub fn query_raw(&self) -> &str {
        &self.query
    }

    /// Deserializes the URL query string into `T`; an absent query is the empty string.
    ///
    /// # Errors
    /// [`ServerError::Deserialize`] (`API_007`, status `400`) when the query does
    /// not match `T` — e.g. a required field is missing or a value is ill-typed.
    pub fn query<T: DeserializeOwned>(&self) -> ServerResult<T> {
        serde_urlencoded::from_str(&self.query)
            .map_err(|error| ServerError::deserialize(error.to_string()))
    }

    /// Parses the captured path parameter `name` into `T`.
    ///
    /// # Errors
    /// - [`ServerError::MissingPathParam`] if the route did not capture `name`.
    /// - [`ServerError::InvalidPathParam`] if the value cannot be parsed into `T`.
    pub fn param<T>(&self, name: &str) -> ServerResult<T>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        self.params.parse(name)
    }

    /// Returns the authenticated principal if one was attached.
    #[must_use]
    pub fn principal_opt(&self) -> Option<&AuthenticatedPrincipal> {
        self.principal.as_ref()
    }

    /// Returns the authenticated principal, requiring one to be present.
    ///
    /// # Errors
    /// Returns [`ServerError::Unauthenticated`] if no principal was attached.
    pub fn principal(&self) -> ServerResult<&AuthenticatedPrincipal> {
        self.principal
            .as_ref()
            .ok_or_else(ServerError::unauthenticated)
    }

    /// Rebuilds this context over the state `Sub` reads out of `S`; the response sinks are shared.
    #[must_use]
    pub fn project<Sub: FromRef<S>>(self) -> HttpContext<Sub> {
        let Self {
            state,
            request_id,
            query,
            params,
            principal,
            locale,
            preferences,
            headers,
            response_headers,
            response_status,
            response_redirect,
            route_template,
            method,
            address,
            asset_addresses,
            peer,
            body_stream,
            response_stream,
            authorized,
            segments,
            shown_error,
            action_routes,
            extensions,
        } = self;
        HttpContext {
            state: Sub::from_ref(&state),
            request_id,
            query,
            params,
            principal,
            locale,
            preferences,
            headers,
            response_headers,
            response_status,
            response_redirect,
            route_template,
            method,
            address,
            asset_addresses,
            peer,
            body_stream,
            response_stream,
            authorized,
            segments,
            shown_error,
            action_routes,
            extensions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_auth::{AuthenticatedPrincipal, UserId};

    #[derive(Clone)]
    struct State {
        name: &'static str,
    }

    fn principal() -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            user_id: UserId::new("user-1"),
            device_id: None,
            scopes: vec!["read".to_owned()],
            expires_at: 0,
        }
    }

    #[test]
    fn exposes_typed_state_and_request_id() {
        let ctx = HttpContext::new(State { name: "db" }, "req-1");
        assert_eq!(ctx.state().name, "db");
        assert_eq!(ctx.request_id(), "req-1");
    }

    #[test]
    fn param_reads_from_attached_path_params() {
        let params = [("id".to_owned(), "7".to_owned())].into_iter().collect();
        let ctx = HttpContext::new(State { name: "db" }, "req-1").with_params(params);
        let id: u64 = ctx.param("id").expect("parses");
        assert_eq!(id, 7);
    }

    #[test]
    fn principal_absent_is_unauthenticated() {
        let ctx = HttpContext::new(State { name: "db" }, "req-1");
        assert!(ctx.principal_opt().is_none());
        let err = ctx.principal().expect_err("no principal");
        assert_eq!(err.error_code(), "API_009");
        assert_eq!(err.http_status_code(), 401);
    }

    #[test]
    fn principal_present_is_returned() {
        let ctx = HttpContext::new(State { name: "db" }, "req-1").with_principal(Some(principal()));
        assert_eq!(
            ctx.principal().expect("present").user_id,
            UserId::new("user-1")
        );
    }

    #[test]
    fn header_reads_case_insensitively_from_attached_headers() {
        let headers: RequestHeaders = [("DPoP".to_owned(), "proof-jws".to_owned())]
            .into_iter()
            .collect();
        let ctx = HttpContext::new(State { name: "db" }, "req-1").with_headers(headers);
        assert_eq!(ctx.header("DPoP"), Some("proof-jws"));
        assert_eq!(ctx.header("dpop"), Some("proof-jws"));
        assert_eq!(ctx.header("authorization"), None);
    }

    #[test]
    fn header_absent_when_no_headers_attached() {
        let ctx = HttpContext::new(State { name: "db" }, "req-1");
        assert!(ctx.header("dpop").is_none());
        assert!(ctx.headers().is_empty());
    }

    fn with_cookies(fields: &[&str]) -> HttpContext<State> {
        let headers: RequestHeaders = fields
            .iter()
            .map(|field| ("Cookie".to_owned(), (*field).to_owned()))
            .collect();
        HttpContext::new(State { name: "db" }, "req-1").with_headers(headers)
    }

    #[test]
    fn a_form_with_or_without_a_file_is_a_form_submission_and_json_is_not() {
        let posted = |content_type: &str| {
            let headers: RequestHeaders = [("Content-Type".to_owned(), content_type.to_owned())]
                .into_iter()
                .collect();
            HttpContext::new(State { name: "db" }, "req-1")
                .with_headers(headers)
                .is_form_submission()
        };
        assert!(posted("application/x-www-form-urlencoded; charset=UTF-8"));
        assert!(posted("multipart/form-data; boundary=----x"));
        assert!(posted("Multipart/Form-Data; boundary=x"));
        assert!(!posted("application/json"));
        assert!(!HttpContext::new(State { name: "db" }, "req-1").is_form_submission());
    }

    #[test]
    fn cookie_reads_a_named_value() {
        let ctx = with_cookies(&["theme=dark; locale=nl"]);
        assert_eq!(ctx.cookie("theme"), Some("dark"));
        assert_eq!(ctx.cookie("locale"), Some("nl"));
        assert_eq!(ctx.cookie("absent"), None);
    }

    #[test]
    fn cookie_reads_across_split_cookie_fields() {
        let ctx = with_cookies(&["theme=dark", "locale=nl"]);
        assert_eq!(ctx.cookie("locale"), Some("nl"));
        assert_eq!(
            ctx.header("cookie"),
            Some("theme=dark"),
            "the raw header is the first field only — which is exactly the trap"
        );
    }

    /// Cookie names are case-sensitive, unlike header names.
    #[test]
    fn cookie_names_are_case_sensitive() {
        let ctx = with_cookies(&["Theme=dark"]);
        assert_eq!(ctx.cookie("theme"), None);
        assert_eq!(ctx.cookie("Theme"), Some("dark"));
    }

    #[test]
    fn an_ambiguous_cookie_reads_as_absent() {
        let ctx = with_cookies(&["theme=dark; theme=light"]);
        assert_eq!(ctx.cookie("theme"), None);
    }

    #[test]
    fn a_context_carries_no_locale_until_the_boundary_resolves_one() {
        let ctx: HttpContext<()> = HttpContext::new((), "req-1");
        assert_eq!(ctx.locale(), None);
    }

    #[test]
    fn the_resolved_locale_reaches_the_handler() {
        let dutch: Locale = "nl-BE".parse().expect("a language tag");
        let ctx: HttpContext<()> = HttpContext::new((), "req-1").with_locale(Some(dutch.clone()));
        assert_eq!(ctx.locale(), Some(&dutch));
    }

    #[test]
    fn a_recorded_redirect_reaches_the_router() {
        let ctx: HttpContext<()> = HttpContext::new((), "req-1");
        ctx.redirect(Redirect::see_other("/login").expect("valid"));
        let recorded = ctx.response_redirect().take().expect("a redirect");
        assert_eq!(recorded.status(), 303);
        assert_eq!(recorded.location(), "/login");
    }

    // A metrics middleware reads the status sink, so every redirect would otherwise count as a success.
    #[test]
    fn a_redirect_is_mirrored_into_the_status_sink_for_observers() {
        let ctx: HttpContext<()> = HttpContext::new((), "req-1");
        ctx.redirect(Redirect::see_other("/login").expect("valid"));
        assert_eq!(ctx.response_status().get(), Some(303));
    }

    #[test]
    fn a_page_is_spelled_at_the_requests_address() {
        use fundamental_core::address::{LocalePrefixSet, PageSlug, SlugTable};
        let page = PagePath::of_route("/why").with_query("refused", "ACCT_004");
        let bare: HttpContext<()> = HttpContext::new((), "req-1");
        assert_eq!(bare.href_to(&page), "/why?refused=ACCT_004");

        let slugs = SlugTable::builder()
            .page("/why", &[PageSlug::current("nl", "waarom")])
            .build()
            .expect("well formed");
        let prefixes = LocalePrefixSet::new()
            .with("nl", "nl-NL".parse().expect("tag"))
            .expect("nl");
        let space = Arc::new(AddressSpace::new(prefixes, slugs));
        let address = RequestAddress::new(space, "nl-NL".parse().expect("tag"), "/nl", "/");
        let dutch: HttpContext<()> = HttpContext::new((), "req-1").with_address(Some(address));
        assert_eq!(dutch.href_to(&page), "/nl/waarom?refused=ACCT_004");
    }

    #[test]
    fn redirecting_records_no_location_header_of_its_own() {
        let ctx: HttpContext<()> = HttpContext::new((), "req-1");
        ctx.redirect(Redirect::see_other("/first").expect("valid"));
        ctx.redirect(Redirect::see_other("/second").expect("valid"));
        assert!(
            ctx.response_headers().take().is_empty(),
            "the header sink stays for headers the handler chose itself"
        );
    }
}
