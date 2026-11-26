use std::sync::{Arc, Mutex};

#[cfg(feature = "ssr")]
use fundamental_core::AnyError;
use fundamental_core::address::{AddressSpace, PageLanguage, PagePath, RequestAddress};
use fundamental_core::asset::{AssetAddresses, AssetUrl};
use fundamental_core::error::ShownError;
use fundamental_core::i18n::Locale;
use fundamental_core::identity::AuthenticatedPrincipal;
use tracing::warn;

use crate::page_head::PageHead;

/// The request data a page reads while it renders, and the answer it sends back.
///
/// It holds no server types, so a page file reading it compiles for both builds.
///
/// ```
/// # extern crate fundamental_web;
/// use fundamental_web::prelude::*;
/// use fundamental_web::PageContext;
///
/// # fn page_body() {
/// let ctx = use_context::<PageContext>().expect("inside a page render");
/// let id = ctx.param("id").unwrap_or("");
/// let greeting = match ctx.principal() {
///     Some(principal) => format!("Welcome back, {}", principal.user_id),
///     None => "Sign in".to_owned(),
/// };
/// # let _ = (id, greeting);
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct PageContext {
    params: Vec<(String, String)>,
    query: String,
    request_id: String,
    principal: Option<AuthenticatedPrincipal>,
    locale: Option<Locale>,
    preferences: Vec<(&'static str, &'static str)>,
    address: Option<RequestAddress>,
    asset_addresses: Arc<AssetAddresses>,
    live_reload: bool,
    shown_error: Option<ShownError>,
    // Shared handles: a page reads this type by value, and every clone must write to one place.
    status: Arc<Mutex<Option<u16>>>,
    headers: Arc<Mutex<Vec<(String, String)>>>,
    head: Arc<Mutex<PageHead>>,
    // The first failure of a blocking query, which answers the whole page in place of it.
    #[cfg(feature = "ssr")]
    refusal: Arc<Mutex<Option<AnyError>>>,
    #[cfg(feature = "ssr")]
    document: Option<Arc<crate::Document>>,
}

impl PageContext {
    /// Creates a page context over the matched `params` and the `request_id`.
    #[must_use]
    pub fn new(params: Vec<(String, String)>, request_id: impl Into<String>) -> Self {
        Self {
            params,
            query: String::new(),
            request_id: request_id.into(),
            principal: None,
            locale: None,
            preferences: Vec::new(),
            address: None,
            asset_addresses: Arc::default(),
            live_reload: false,
            shown_error: None,
            status: Arc::new(Mutex::new(None)),
            headers: Arc::new(Mutex::new(Vec::new())),
            head: Arc::new(Mutex::new(PageHead::default())),
            #[cfg(feature = "ssr")]
            refusal: Arc::new(Mutex::new(None)),
            #[cfg(feature = "ssr")]
            document: None,
        }
    }

    /// Attaches the shell the site installed — see [`Document::install`](crate::Document::install).
    #[cfg(feature = "ssr")]
    #[must_use]
    pub fn with_document(mut self, document: Option<Arc<crate::Document>>) -> Self {
        self.document = document;
        self
    }

    /// The shell the site installed, which [`default_document`](crate::default_document) wraps the page in.
    #[cfg(feature = "ssr")]
    #[must_use]
    pub fn document(&self) -> Option<&crate::Document> {
        self.document.as_deref()
    }

    /// The language this page is answered in, with the region the site writes it in (`en-GB`).
    #[cfg(feature = "ssr")]
    #[must_use]
    pub fn regional_locale(&self) -> Option<Locale> {
        let locale = self.locale.as_ref()?;
        Some(
            self.document
                .as_ref()
                .map_or_else(|| locale.clone(), |document| document.regional(locale)),
        )
    }

    /// Attaches the URL query the request carried, without the leading `?`.
    #[must_use]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self
    }

    /// Attaches the caller the request authenticated as, if any.
    #[must_use]
    pub fn with_principal(mut self, principal: Option<AuthenticatedPrincipal>) -> Self {
        self.principal = principal;
        self
    }

    /// Attaches the language this request is answered in, if the surface negotiates.
    #[must_use]
    pub fn with_locale(mut self, locale: Option<Locale>) -> Self {
        self.locale = locale;
        self
    }

    /// Attaches the versions of the router's assets, for [`asset_url`](Self::asset_url).
    #[must_use]
    pub fn with_asset_addresses(mut self, asset_addresses: Arc<AssetAddresses>) -> Self {
        self.asset_addresses = asset_addresses;
        self
    }

    /// Says whether the router serves the development live reload.
    #[must_use]
    pub fn with_live_reload(mut self, live_reload: bool) -> Self {
        self.live_reload = live_reload;
        self
    }

    /// Whether the router serves the development live reload.
    #[must_use]
    pub fn live_reload(&self) -> bool {
        self.live_reload
    }

    /// The address to link the router's asset at `path` by: `path?v=<version>`, or the bare `path`.
    #[must_use]
    pub fn asset_url(&self, path: &str) -> AssetUrl {
        self.asset_addresses.url(path)
    }

    /// Attaches the error this page is answering — the router's error page only.
    #[must_use]
    pub fn with_shown_error(mut self, shown_error: Option<ShownError>) -> Self {
        self.shown_error = shown_error;
        self
    }

    /// The error this page is answering; `Some` only on the app's error page.
    #[must_use]
    pub fn shown_error(&self) -> Option<ShownError> {
        self.shown_error
    }

    /// Attaches the display preferences this reader pinned.
    #[must_use]
    pub fn with_preferences(mut self, preferences: Vec<(&'static str, &'static str)>) -> Self {
        self.preferences = preferences;
        self
    }

    /// The value this reader pinned for `name`, if they pinned one this build offers.
    ///
    /// A closed value, never the request's cookies, so a browser bundle cannot read a session.
    #[must_use]
    pub fn preference(&self, name: &str) -> Option<&'static str> {
        self.preferences
            .iter()
            .find(|(pinned, _)| *pinned == name)
            .map(|(_, value)| *value)
    }

    /// Attaches the address this page is being rendered at, if the surface has an address space.
    #[must_use]
    pub fn with_address(mut self, address: Option<RequestAddress>) -> Self {
        self.address = address;
        self
    }

    /// The raw value of the matched path parameter `name`, if the route captured it.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The raw URL query string, without the `?`; empty when there is none.
    #[must_use]
    pub fn query_raw(&self) -> &str {
        &self.query
    }

    /// The request's correlation id, for stitching a page render to its logs.
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Who is signed in, or `None`; a courtesy for drawing, not a permission check.
    #[must_use]
    pub fn principal(&self) -> Option<&AuthenticatedPrincipal> {
        self.principal.as_ref()
    }

    /// The language to answer in, or `None` when the surface negotiates none.
    #[must_use]
    pub fn locale(&self) -> Option<&Locale> {
        self.locale.as_ref()
    }

    /// Puts this context, and the address it is answered at, in the rendering owner's context.
    pub fn provide(self) {
        if let Some(address) = self.address.clone() {
            leptos::prelude::provide_context(address);
        }
        leptos::prelude::provide_context(self);
    }

    /// The page in each language it is published in, for a language switch.
    #[must_use]
    pub fn languages(&self) -> Vec<PageLanguage> {
        self.address
            .as_ref()
            .map(RequestAddress::languages)
            .unwrap_or_default()
    }

    /// The address this page is being rendered at, on a surface that has an address space.
    #[must_use]
    pub fn address(&self) -> Option<&RequestAddress> {
        self.address.as_ref()
    }

    /// The page a route builder named, spelled in this page's own locale; the route path without addressing.
    #[must_use]
    pub fn href_to(&self, page: &PagePath) -> String {
        match &self.address {
            Some(address) => address.href_to(page),
            None => AddressSpace::default().page_address("", page),
        }
    }

    /// Answers with `status` rather than `200`; the last call wins.
    pub fn set_response_status(&self, status: u16) {
        // A poisoned lock degrades to `200` rather than panicking a render.
        if let Ok(mut slot) = self.status.lock() {
            *slot = Some(status);
        }
    }

    /// Adds a response header, appending rather than replacing.
    pub fn set_response_header(&self, name: impl Into<String>, value: impl Into<String>) {
        if let Ok(mut headers) = self.headers.lock() {
            headers.push((name.into(), value.into()));
        }
    }

    /// Sets the document title; the last call wins.
    pub fn set_title(&self, title: impl Into<String>) {
        self.with_head(|head| head.title = Some(title.into()));
    }

    /// Sets the `meta name="description"` — the sentence a search result shows.
    pub fn set_description(&self, description: impl Into<String>) {
        self.with_head(|head| head.description = Some(description.into()));
    }

    /// Sets the `link rel="canonical"` href.
    pub fn set_canonical(&self, href: impl Into<String>) {
        self.with_head(|head| head.canonical = Some(href.into()));
    }

    /// Adds a `meta name="…" content="…"` pair, appending rather than replacing.
    pub fn add_meta(&self, name: impl Into<String>, content: impl Into<String>) {
        let pair = (name.into(), content.into());
        self.with_head(|head| head.meta.push(pair));
    }

    /// Adds a `meta property="…" content="…"` pair: Open Graph is defined on `property`, not `name`.
    pub fn add_meta_property(&self, property: impl Into<String>, content: impl Into<String>) {
        let pair = (property.into(), content.into());
        self.with_head(|head| head.meta_properties.push(pair));
    }

    /// Adds a `link rel="…" href="…"` pair, appending rather than replacing.
    pub fn add_link(&self, rel: impl Into<String>, href: impl Into<String>) {
        let pair = (rel.into(), href.into());
        self.with_head(|head| head.links.push(pair));
    }

    /// Runs `module` on this page: its script and a preload for each module beside it.
    ///
    /// A page runs one module graph, so a second call replaces the first, logged.
    pub fn add_module(&self, module: impl Into<crate::ModuleScript>) {
        let module = module.into();
        self.with_head(|head| {
            if head.module.as_ref().is_some_and(|held| *held != module) {
                warn!(
                    src = module.src(),
                    "a second module script replaced the page's first"
                );
            }
            head.module = Some(module);
        });
    }

    /// Adds a `link rel="alternate" hreflang="…" href="…"`; prefer [`RequestAddress::alternates`](fundamental_core::address::RequestAddress::alternates).
    pub fn add_alternate(&self, hreflang: impl Into<String>, href: impl Into<String>) {
        let pair = (hreflang.into(), href.into());
        self.with_head(|head| head.alternates.push(pair));
    }

    /// Adds a JSON-LD object (schema.org) to the head; a non-object is refused, logged.
    pub fn add_structured_data(&self, value: &serde_json::Value) {
        if !value.is_object() {
            warn!("structured data that is not a JSON object was not added to the head");
            return;
        }
        let json = value.to_string();
        self.with_head(|head| head.structured_data.push(json));
    }

    /// Adds a schema.org object about this page's subject, its `@id` and `url` the canonical URL.
    pub fn add_structured_data_about(
        &self,
        schema_type: &str,
        mut fields: serde_json::Map<String, serde_json::Value>,
    ) {
        let Some(url) = self.address().and_then(RequestAddress::canonical_url) else {
            warn!(
                schema_type,
                "structured data needs the page's canonical URL; none, so none added"
            );
            return;
        };
        let url = url.into_string();
        fields.insert("@context".to_owned(), "https://schema.org".into());
        fields.insert("@type".to_owned(), schema_type.into());
        fields.insert("@id".to_owned(), url.clone().into());
        fields.insert("url".to_owned(), url.into());
        self.add_structured_data(&serde_json::Value::Object(fields));
    }

    /// A snapshot of what the page contributed to `<head>`.
    #[must_use]
    pub fn head(&self) -> PageHead {
        self.head
            .lock()
            .map(|head| head.clone())
            .unwrap_or_default()
    }

    /// Applies `edit` to the head sink, or drops it if another thread poisoned the lock.
    fn with_head(&self, edit: impl FnOnce(&mut PageHead)) {
        if let Ok(mut head) = self.head.lock() {
            edit(&mut head);
        }
    }

    /// Takes the status the page recorded, leaving none behind.
    #[cfg(feature = "ssr")]
    pub(crate) fn take_response_status(&self) -> Option<u16> {
        self.status.lock().ok().and_then(|mut slot| slot.take())
    }

    /// Records that a blocking query failed with `error`; the first failure is the one answered.
    #[cfg(feature = "ssr")]
    pub(crate) fn refuse(&self, error: AnyError) {
        if let Ok(mut slot) = self.refusal.lock() {
            slot.get_or_insert(error);
        }
    }

    /// Takes the failure a blocking query recorded, leaving none behind.
    #[cfg(feature = "ssr")]
    pub(crate) fn take_refusal(&self) -> Option<AnyError> {
        self.refusal.lock().ok().and_then(|mut slot| slot.take())
    }

    /// Takes the headers the page recorded, leaving none behind.
    #[cfg(feature = "ssr")]
    pub(crate) fn take_response_headers(&self) -> Vec<(String, String)> {
        self.headers
            .lock()
            .map(|mut headers| std::mem::take(&mut *headers))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::identity::UserId;

    fn context() -> PageContext {
        PageContext::new(vec![("id".to_owned(), "42".to_owned())], "req-1")
    }

    #[test]
    fn a_bare_context_knows_its_request_and_nothing_about_its_reader() {
        let ctx = context();
        assert_eq!(ctx.param("id"), Some("42"));
        assert_eq!(ctx.param("missing"), None);
        assert_eq!(ctx.request_id(), "req-1");
        assert!(ctx.principal().is_none());
        assert!(ctx.locale().is_none());
        assert_eq!(ctx.query_raw(), "");
    }

    #[test]
    fn a_page_path_prints_through_the_address_or_as_written() {
        let page = PagePath::of_route("/why").with_query("next", "/a");
        assert_eq!(context().href_to(&page), "/why?next=%2Fa");
        let slugs = fundamental_core::address::SlugTable::builder()
            .page(
                "/why",
                &[fundamental_core::address::PageSlug::current("nl", "waarom")],
            )
            .build()
            .expect("well formed");
        let prefixes = fundamental_core::address::LocalePrefixSet::new()
            .with("nl", "nl-NL".parse().expect("tag"))
            .expect("nl");
        let space = Arc::new(AddressSpace::new(prefixes, slugs));
        let address = RequestAddress::new(space, "nl-NL".parse().expect("tag"), "/nl", "/");
        assert_eq!(
            context().with_address(Some(address)).href_to(&page),
            "/nl/waarom?next=%2Fa"
        );
    }

    #[test]
    fn the_query_is_readable_verbatim_once_attached() {
        // Verbatim: a page echoes it into an `href`, and re-encoding would double-encode.
        let ctx = context().with_query("next=%2Fclients&page=2");
        assert_eq!(ctx.query_raw(), "next=%2Fclients&page=2");
    }

    #[test]
    fn the_reader_and_the_language_are_readable_once_attached() {
        let dutch: Locale = "nl".parse().expect("a language tag");
        let ctx = context()
            .with_principal(Some(AuthenticatedPrincipal {
                user_id: UserId::new("u1"),
                device_id: None,
                scopes: vec!["portal:read".to_owned()],
                expires_at: 0,
            }))
            .with_locale(Some(dutch.clone()));

        assert_eq!(
            ctx.principal().map(|principal| principal.user_id.as_str()),
            Some("u1")
        );
        assert!(
            ctx.principal()
                .is_some_and(|principal| principal.has_scope("portal:read"))
        );
        assert_eq!(ctx.locale(), Some(&dutch));
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn a_recorded_status_is_taken_once() {
        let ctx = context();
        assert_eq!(ctx.take_response_status(), None);
        ctx.set_response_status(404);
        assert_eq!(ctx.take_response_status(), Some(404));
        assert_eq!(
            ctx.take_response_status(),
            None,
            "the sink is drained, not read"
        );
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_last_status_wins() {
        let ctx = context();
        ctx.set_response_status(404);
        ctx.set_response_status(410);
        assert_eq!(ctx.take_response_status(), Some(410));
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn headers_append_and_keep_their_order() {
        let ctx = context();
        ctx.set_response_header("Link", "</a.css>; rel=preload");
        ctx.set_response_header("Link", "</b.css>; rel=preload");
        assert_eq!(
            ctx.take_response_headers(),
            [
                ("Link".to_owned(), "</a.css>; rel=preload".to_owned()),
                ("Link".to_owned(), "</b.css>; rel=preload".to_owned()),
            ]
        );
        assert!(ctx.take_response_headers().is_empty());
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_first_refusal_is_the_one_taken_once() {
        use fundamental_server::ServerError;

        let ctx = context();
        assert!(ctx.take_refusal().is_none());
        ctx.clone()
            .refuse(AnyError::new(ServerError::Unauthenticated(
                "first".to_owned(),
            )));
        ctx.refuse(AnyError::new(ServerError::internal("second")));
        let taken = ctx.take_refusal().expect("a refusal was recorded");
        assert_eq!(taken.get().http_status_code(), 401);
        assert!(ctx.take_refusal().is_none());
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn a_clone_writes_to_the_same_sink() {
        let ctx = context();
        let clone = ctx.clone();
        clone.set_response_status(404);
        clone.set_response_header("X-Test", "1");
        assert_eq!(ctx.take_response_status(), Some(404));
        assert_eq!(ctx.take_response_headers().len(), 1);
    }
}
