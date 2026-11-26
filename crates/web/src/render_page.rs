use std::sync::Arc;

use fundamental_core::address::RequestAddress;
use fundamental_server::HttpContext;
use leptos::prelude::*;

use crate::in_language::in_language;
use crate::page_context::PageContext;
use crate::render_options::RenderOptions;
use crate::rendered_page::RenderedPage;

/// Renders `view_fn`'s view to body HTML, with the request's [`PageContext`] and state in context.
///
/// The render runs inside the request's translator, and the status and headers the page
/// recorded are handed to `ctx`.
pub fn render_page<S, F, V>(ctx: &HttpContext<S>, view_fn: F) -> RenderedPage
where
    S: Clone + Send + Sync + 'static,
    F: FnOnce() -> V,
    V: RenderHtml + 'static,
{
    render_with(ctx, RenderOptions::default(), || {}, view_fn)
}

/// The render itself, with `provide` run inside the owner before the view; the one render every entry shares.
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
pub(crate) fn render_with<S, P, F, V>(
    ctx: &HttpContext<S>,
    options: RenderOptions,
    provide: P,
    view_fn: F,
) -> RenderedPage
where
    S: Clone + Send + Sync + 'static,
    P: FnOnce(),
    F: FnOnce() -> V,
    V: RenderHtml + 'static,
{
    let params = ctx
        .params()
        .iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    let page_context = PageContext::new(params, ctx.request_id())
        .with_query(ctx.query_raw())
        .with_principal(ctx.principal_opt().cloned())
        .with_locale(ctx.locale().cloned())
        .with_preferences(ctx.preferences().to_vec())
        .with_address(entity_address(ctx))
        .with_asset_addresses(Arc::clone(ctx.asset_addresses()))
        .with_live_reload(ctx.serves_live_reload())
        .with_document(ctx.extension::<Arc<crate::Document>>().cloned())
        .with_shown_error(ctx.shown_error());
    let state = (!options.without_state).then(|| ctx.state().clone());
    let title = options.title;

    let render = {
        let page_context = page_context.clone();
        move || {
            let owner = Owner::new();
            owner.with(|| {
                if let Some(title) = title {
                    page_context.set_title(title.to_string());
                }
                page_context.provide();
                if let Some(state) = state {
                    provide_context(state);
                }
                provide();
                view_fn().to_html()
            })
        }
    };
    let body = in_language(ctx.locale(), render);

    // The page's sinks are wasm-clean, not the server's; hand them over while the request is in scope.
    let status = page_context.take_response_status();
    if let Some(status) = status {
        ctx.set_response_status(status);
    }
    for (name, value) in page_context.take_response_headers() {
        ctx.set_response_header(name, value);
    }
    let undisclosed =
        ctx.segments().is_declared() && !ctx.segments().disclosable(ctx.authorized_objects());
    if status.is_none_or(|status| (200..300).contains(&status)) && !undisclosed {
        name_the_address(&page_context);
    }

    RenderedPage::new(page_context, body)
}

/// The address the page names itself by: the entity's canonical path once authorization affirmed it.
///
/// Until then, and on a refusal, the address names nothing, so no canonical URL leaks.
pub(super) fn entity_address<S>(ctx: &HttpContext<S>) -> Option<RequestAddress> {
    let address = ctx.address()?.clone();
    let refused = ctx.shown_error().is_some()
        || ctx
            .response_status()
            .get()
            .is_some_and(|status| !(200..300).contains(&status));
    if refused {
        return Some(address.unserved());
    }
    let segments = ctx.segments();
    if !segments.is_declared() {
        return Some(address);
    }
    if !segments.disclosable(ctx.authorized_objects()) {
        return Some(address.unserved());
    }
    match ctx
        .route_template()
        .and_then(|template| segments.language_paths(template))
    {
        Some(paths) => Some(address.with_language_paths(paths)),
        None => Some(address),
    }
}

/// Writes the page's canonical URL and `hreflang` alternates into its head unless it wrote its own; skipped outside `2xx`.
pub(super) fn name_the_address(page: &PageContext) {
    let Some(address) = page.address() else {
        return;
    };
    let head = page.head();
    if head.canonical.is_none()
        && let Some(url) = address.canonical_url()
    {
        page.set_canonical(url.into_string());
    }
    if head.alternates.is_empty()
        && let Some(alternates) = address.alternate_urls()
    {
        for (hreflang, url) in alternates {
            page.add_alternate(hreflang, url.into_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use std::sync::Arc;

    use fundamental_auth::{AuthenticatedPrincipal, UserId};
    use fundamental_core::i18n::{self, Arg, Locale, Translator, TranslatorSource};

    #[derive(Clone)]
    struct AppState {
        greeting: &'static str,
    }

    fn context() -> HttpContext<AppState> {
        let params = [("id".to_owned(), "42".to_owned())].into_iter().collect();
        HttpContext::new(AppState { greeting: "Hi" }, "req-1").with_params(params)
    }

    fn locale(tag: &str) -> Locale {
        tag.parse().expect("a language tag")
    }

    #[test]
    fn the_page_context_links_the_routers_assets_by_version() {
        use fundamental_core::asset::{AssetAddresses, AssetVersion};
        let version = AssetVersion::parse("q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA")
            .expect("a valid version");
        let mut addresses = AssetAddresses::default();
        addresses.insert("/_f/app.css", version.clone());
        let ctx = context().with_asset_addresses(Arc::new(addresses));
        let rendered = render_page(&ctx, || {
            let page = use_context::<PageContext>().expect("page context");
            let css = page.asset_url("/_f/app.css").to_string();
            let other = page.asset_url("/_f/app.js").to_string();
            view! { <link rel="stylesheet" href=css /><script src=other></script> }
        });
        let html = rendered.body();
        assert!(
            html.contains(&format!("/_f/app.css?v={version}")),
            "rendered: {html}"
        );
        assert!(html.contains("src=\"/_f/app.js\""), "rendered: {html}");
    }

    #[test]
    fn a_page_reads_state_and_params_from_context() {
        let rendered = render_page(&context(), || {
            let state = use_context::<AppState>().expect("state in context");
            let page = use_context::<PageContext>().expect("page context");
            let id = page.param("id").unwrap_or("").to_owned();
            view! { <p>{state.greeting}" #"{id}</p> }
        });
        let html = rendered.body();
        assert!(html.contains("Hi"), "rendered: {html}");
        assert!(html.contains("42"), "rendered: {html}");
    }

    #[island]
    fn Counter(start: i32) -> impl IntoView {
        let (count, set) = signal(start);
        view! { <button on:click=move |_| set.update(|v| *v += 1)>{move || count.get()}</button> }
    }

    #[test]
    fn an_island_renders_as_a_hydration_marker_with_props() {
        let rendered = render_page(&context(), || view! { <main><Counter start=5 /></main> });
        let html = rendered.body();
        assert!(html.contains("<leptos-island"), "rendered: {html}");
        assert!(html.contains("data-props="), "rendered: {html}");
        assert!(
            html.contains("start") && html.contains('5'),
            "props: {html}"
        );
        assert!(html.contains("<button"), "rendered: {html}");
    }

    #[test]
    fn each_render_gets_its_own_context() {
        // A second render must not see the first's context: the owner is request-scoped.
        let _ = render_page(&context(), || view! { <span>"first"</span> });
        let rendered = render_page(&context(), || {
            let page = use_context::<PageContext>().expect("page context");
            view! { <span>{page.request_id().to_owned()}</span> }
        });
        assert!(rendered.body().contains("req-1"), "rendered: {rendered:?}");
    }

    #[test]
    fn the_requests_query_reaches_the_page() {
        let ctx = context().with_query("next=%2Fclients&page=2");
        let rendered = render_page(&ctx, || {
            let page = use_context::<PageContext>().expect("page context");
            view! { <span>{page.query_raw().to_owned()}</span> }
        });
        assert!(
            rendered.body().contains("next=%2Fclients&amp;page=2"),
            "rendered: {rendered:?}"
        );
    }

    #[test]
    fn a_page_requested_without_a_query_reads_an_empty_one() {
        // Leptos writes hydration markers between text nodes, so an empty string is not observable in the body.
        let rendered = render_page(&context(), || view! { <span>"ok"</span> });
        assert_eq!(rendered.context().query_raw(), "");
    }

    #[test]
    fn the_reader_reaches_the_page() {
        let ctx = context().with_principal(Some(AuthenticatedPrincipal {
            user_id: UserId::new("u1"),
            device_id: None,
            scopes: Vec::new(),
            expires_at: 0,
        }));
        let rendered = render_page(&ctx, || {
            let page = use_context::<PageContext>().expect("page context");
            let who = page
                .principal()
                .map_or_else(|| "anonymous".to_owned(), |p| p.user_id.to_string());
            view! { <span>{who}</span> }
        });
        assert!(rendered.body().contains("u1"), "rendered: {rendered:?}");
    }

    #[test]
    fn a_signed_out_visitor_reaches_the_same_page_with_no_principal() {
        let rendered = render_page(&context(), || {
            let page = use_context::<PageContext>().expect("page context");
            let who = page
                .principal()
                .map_or_else(|| "anonymous".to_owned(), |p| p.user_id.to_string());
            view! { <span>{who}</span> }
        });
        assert!(
            rendered.body().contains("anonymous"),
            "rendered: {rendered:?}"
        );
    }

    #[test]
    fn the_negotiated_locale_reaches_the_page_and_comes_back_out_for_the_shell() {
        let ctx = context().with_locale(Some(locale("nl")));
        let rendered = render_page(&ctx, || {
            let page = use_context::<PageContext>().expect("page context");
            let lang = page.locale().map_or_else(String::new, Locale::to_string);
            view! { <span>{lang}</span> }
        });
        assert!(rendered.body().contains("nl"), "rendered: {rendered:?}");
        assert_eq!(rendered.context().locale(), Some(&locale("nl")));
    }

    #[test]
    fn a_page_that_resolved_nothing_answers_with_its_own_status_and_headers() {
        let ctx = context();
        let rendered = render_page(&ctx, || {
            let page = use_context::<PageContext>().expect("page context");
            page.set_response_status(404);
            page.set_response_header("X-Robots-Tag", "noindex");
            view! { <p>"Not found"</p> }
        });
        assert!(rendered.body().contains("Not found"));
        assert_eq!(
            ctx.response_status().take(),
            Some(404),
            "the router reads the status off the request context, not off the page's"
        );
        assert_eq!(
            ctx.response_headers().take(),
            [("X-Robots-Tag".to_owned(), "noindex".to_owned())]
        );
    }

    #[test]
    fn a_page_that_sets_nothing_leaves_the_response_alone() {
        let ctx = context();
        let _ = render_page(&ctx, || view! { <p>"ok"</p> });
        assert_eq!(ctx.response_status().take(), None);
        assert!(ctx.response_headers().take().is_empty());
    }

    struct DutchTranslator;
    impl Translator for DutchTranslator {
        fn lookup(&self, key: &str, _args: &[Arg<'_>]) -> Option<String> {
            (key == "greeting").then(|| "Hallo".to_owned())
        }
    }

    struct DutchOnly;
    impl TranslatorSource for DutchOnly {
        fn translator(&self, locale: &Locale) -> Arc<dyn Translator> {
            if locale.language() == "nl" {
                Arc::new(DutchTranslator)
            } else {
                Arc::new(fundamental_core::i18n::NoopTranslator)
            }
        }
    }

    fn greeting() -> String {
        i18n::translate("greeting", &[], || Cow::Borrowed("Hello")).into_owned()
    }

    #[test]
    fn the_render_happens_inside_the_requests_language_and_does_not_leak_out_of_it() {
        i18n::set_translator_source(Arc::new(DutchOnly));

        let ctx = context().with_locale(Some(locale("nl")));
        let rendered = render_page(&ctx, || view! { <p>{greeting()}</p> });
        assert!(rendered.body().contains("Hallo"), "rendered: {rendered:?}");

        assert_eq!(
            greeting(),
            "Hello",
            "the scope is the request's; the next one on this thread must not inherit it"
        );
    }

    #[test]
    fn the_shell_renders_in_the_pages_language_too() {
        i18n::set_translator_source(Arc::new(DutchOnly));

        let ctx = context().with_locale(Some(locale("nl")));
        let document =
            render_page(&ctx, || view! { <p>{greeting()}</p> }).into_document(|page, body| {
                format!(
                    "<nav>{}</nav>{body}<!--{}-->",
                    greeting(),
                    page.request_id()
                )
            });

        assert!(
            document.starts_with("<nav>Hallo</nav>"),
            "document: {document}"
        );
        assert!(document.contains("<p>Hallo</p>"), "document: {document}");
        assert!(
            document.ends_with("<!--req-1-->"),
            "the shell sees the page's context"
        );
        assert_eq!(greeting(), "Hello", "the shell's scope ends with it");
    }

    #[test]
    fn a_request_with_no_locale_renders_the_source_strings() {
        i18n::set_translator_source(Arc::new(DutchOnly));

        let rendered = render_page(&context(), || view! { <p>{greeting()}</p> });
        assert!(rendered.body().contains("Hello"), "rendered: {rendered:?}");
    }

    fn addressed(served: bool) -> HttpContext<AppState> {
        use fundamental_core::address::{
            AddressSpace, LocalePrefixSet, RequestAddress, SlugTable, WebOrigin,
        };
        let prefixes = LocalePrefixSet::new().with("en", locale("en")).expect("en");
        let space = AddressSpace::new(prefixes, SlugTable::default())
            .with_default_language(locale("nl"))
            .expect("nl")
            .with_origin(WebOrigin::parse("https://shop.example.nl").expect("origin"));
        let address = RequestAddress::new(Arc::new(space), locale("en"), "/en", "/prijzen");
        let address = match served {
            true => address,
            false => address.unserved(),
        };
        context().with_address(Some(address))
    }

    #[test]
    fn every_served_page_names_its_absolute_address() {
        let rendered = render_page(&addressed(true), || view! { <p>"prijzen"</p> });
        let head = rendered.context().head();
        assert_eq!(
            head.canonical.as_deref(),
            Some("https://shop.example.nl/en/prijzen")
        );
        assert_eq!(
            head.alternates,
            [
                (
                    "nl".to_owned(),
                    "https://shop.example.nl/prijzen".to_owned()
                ),
                (
                    "en".to_owned(),
                    "https://shop.example.nl/en/prijzen".to_owned()
                ),
                (
                    "x-default".to_owned(),
                    "https://shop.example.nl/prijzen".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn a_page_reads_its_address_where_a_kit_part_can() {
        use fundamental_core::address::RequestAddress;
        let rendered = render_page(&addressed(true), || {
            let prefix = use_context::<RequestAddress>().map(|address| address.prefix().to_owned());
            view! { <p>{prefix}</p> }
        });
        assert!(
            rendered.body().contains("<p>/en</p>"),
            "{}",
            rendered.body()
        );
    }

    #[test]
    fn a_page_no_route_serves_names_no_address() {
        let rendered = render_page(&addressed(false), || view! { <p>"lost"</p> });
        let head = rendered.context().head();
        assert_eq!(head.canonical, None);
        assert!(head.alternates.is_empty());
    }

    #[test]
    fn a_page_answering_not_here_names_no_address() {
        let rendered = render_page(&addressed(true), || {
            use_context::<PageContext>()
                .expect("page context")
                .set_response_status(410);
            view! { <p>"gone"</p> }
        });
        assert_eq!(rendered.context().head().canonical, None);
    }

    #[test]
    fn a_pages_own_canonical_wins() {
        let rendered = render_page(&addressed(true), || {
            use_context::<PageContext>()
                .expect("page context")
                .set_canonical("https://shop.example.nl/en/prijzen?page=2");
            view! { <p>"page two"</p> }
        });
        assert_eq!(
            rendered.context().head().canonical.as_deref(),
            Some("https://shop.example.nl/en/prijzen?page=2")
        );
    }
}
