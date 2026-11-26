use std::sync::Arc;

use fundamental_core::escape;
use fundamental_core::i18n::Locale;
use fundamental_server::{Router, ServerResult, StaticAsset};

use crate::page_context::PageContext;
use crate::share_card::{ShareCard, regional};
use crate::site_icon::SiteIcon;

/// The HTML shell every page of a site wears, declared once and [installed](Self::install).
///
/// ```
/// # extern crate fundamental_web;
/// use fundamental_web::{Document, PageContext, ShareCard};
///
/// fn style_links(_: &PageContext) -> String {
///     "<link rel=\"stylesheet\" href=\"/_fundamental/app.css\">".to_owned()
/// }
///
/// let document = Document::new()
///     .head(style_links)
///     .share(ShareCard::new("Example"));
/// let html = document.render(&PageContext::new(Vec::new(), "req"), "<main></main>");
/// assert!(html.contains("app.css"));
/// ```
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct Document {
    head: Option<fn(&PageContext) -> String>,
    title: Option<&'static str>,
    icons: &'static [SiteIcon],
    theme: Option<&'static str>,
    share: Option<ShareCard>,
    locales: Vec<Locale>,
    bundle: Option<String>,
}

impl Document {
    /// A shell writing only the charset, the viewport, the page's own head and its language.
    pub fn new() -> Self {
        Self::default()
    }

    /// Writes what `links` returns into every page's head, after the viewport.
    pub fn head(mut self, links: fn(&PageContext) -> String) -> Self {
        self.head = Some(links);
        self
    }

    /// The title of a page that names none of its own; a page's own [title](PageContext::set_title) wins.
    pub fn title(mut self, title: &'static str) -> Self {
        self.title = Some(title);
        self
    }

    /// Links each of `icons` from every page's head; [`install`](Self::install) serves each.
    pub fn icons(mut self, icons: &'static [SiteIcon]) -> Self {
        self.icons = icons;
        self
    }

    /// Writes the reader's value of the preference `name` as `<html data-theme="…">`.
    pub fn theme(mut self, name: &'static str) -> Self {
        self.theme = Some(name);
        self
    }

    /// Writes `card` on every page: its Open Graph card and its schema.org `WebPage`.
    pub fn share(mut self, card: ShareCard) -> Self {
        self.share = Some(card);
        self
    }

    /// The site's languages, each with the region it is written for (`en-GB`, `nl-NL`).
    pub fn locales(mut self, locales: Vec<Locale>) -> Self {
        self.locales = locales;
        self
    }

    /// Boots every page's islands from the bundle at `js_path`, linked by version.
    pub fn bundle(mut self, js_path: impl Into<String>) -> Self {
        self.bundle = Some(js_path.into());
        self
    }

    /// Whether a bundle is set — a site sets its own when the app named none.
    #[must_use]
    pub fn has_bundle(&self) -> bool {
        self.bundle.is_some()
    }

    /// The icons this shell links.
    #[must_use]
    pub fn icon_list(&self) -> &'static [SiteIcon] {
        self.icons
    }

    /// `locale` with the region the site writes its language in, or as it is.
    #[must_use]
    pub fn regional(&self, locale: &Locale) -> Locale {
        regional(locale, &self.locales)
    }

    /// Serves each [icon](Self::icons) at its path and hands this shell to every page `router` renders.
    ///
    /// # Errors
    /// [`ServerError::PathTaken`](fundamental_server::ServerError::PathTaken) when something
    /// already serves an icon's path.
    pub fn install<S>(self, router: Router<S>) -> ServerResult<Router<S>>
    where
        S: Clone + Send + Sync + 'static,
    {
        let mut router = router;
        for icon in self.icons {
            router = router.document(
                icon.path(),
                StaticAsset::typed(icon.bytes().to_vec(), icon.content_type()),
            )?;
        }
        Ok(router.with_extension(Arc::new(self)))
    }

    /// Wraps the rendered page `body` in this shell; `body` is trusted and not escaped.
    #[must_use]
    pub fn render(&self, ctx: &PageContext, body: &str) -> String {
        let lang = ctx.locale().map_or("en", Locale::language);
        let theme = self
            .theme
            .and_then(|name| ctx.preference(name))
            .map(|value| format!(" data-theme=\"{}\"", escape(value)))
            .unwrap_or_default();
        let links = self.head.map(|links| links(ctx)).unwrap_or_default();
        let icons: String = self.icons.iter().map(SiteIcon::link).collect();
        if let Some(title) = self.title
            && ctx.head().title.is_none()
        {
            ctx.set_title(title);
        }
        if let Some(card) = &self.share {
            card.write(ctx, &self.locales);
        }
        let page_head = ctx.head().render();
        let boot = self
            .bundle
            .as_deref()
            .map(|bundle| ctx.hydration_script(bundle))
            .unwrap_or_default();
        format!(
            "<!DOCTYPE html><html lang=\"{lang}\"{theme}><head>\
<meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
{links}{icons}{page_head}{boot}</head><body>{body}</body></html>"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICONS: [SiteIcon; 1] = [SiteIcon::new("/icon.svg", "image/svg+xml", b"<svg/>").typed()];

    fn style_links(_: &PageContext) -> String {
        "<link rel=\"stylesheet\" href=\"/app.css\">".to_owned()
    }

    fn page() -> PageContext {
        PageContext::new(Vec::new(), "req")
    }

    #[test]
    fn the_shell_writes_its_links_icons_and_the_page_head_in_order() {
        let ctx = page();
        ctx.set_title("Home");
        let html = Document::new()
            .head(style_links)
            .icons(&ICONS)
            .render(&ctx, "<main></main>");
        let links = html.find("app.css").expect("the style links");
        let icon = html.find("/icon.svg").expect("the icon");
        let title = html.find("<title>Home</title>").expect("the title");
        assert!(links < icon && icon < title, "{html}");
        assert!(
            html.ends_with("<body><main></main></body></html>"),
            "{html}"
        );
    }

    #[test]
    fn a_page_that_names_no_title_gets_the_sites() {
        let document = Document::new()
            .title("Example")
            .share(ShareCard::new("Example"));
        let untitled = document.render(&page(), "");
        assert!(untitled.contains("<title>Example</title>"), "{untitled}");
        assert!(
            untitled.contains(r#"<meta property="og:title" content="Example">"#),
            "{untitled}"
        );
        let ctx = page();
        ctx.set_title("Mission");
        let titled = document.render(&ctx, "");
        assert!(titled.contains("<title>Mission</title>"), "{titled}");
        assert!(!titled.contains("<title>Example</title>"), "{titled}");
    }

    #[test]
    fn a_pinned_theme_is_written_on_the_root_element() {
        let ctx = page().with_preferences(vec![("appearance", "dark")]);
        let html = Document::new().theme("appearance").render(&ctx, "");
        assert!(
            html.starts_with("<!DOCTYPE html><html lang=\"en\" data-theme=\"dark\">"),
            "{html}"
        );
    }

    #[test]
    fn no_pinned_theme_writes_no_attribute() {
        let html = Document::new().theme("appearance").render(&page(), "");
        assert!(
            html.starts_with("<!DOCTYPE html><html lang=\"en\"><head>"),
            "{html}"
        );
    }

    #[test]
    fn the_bundle_boots_the_islands_by_version() {
        let html = Document::new()
            .bundle("/_fundamental/app.js")
            .render(&page(), "");
        assert!(
            html.contains("data-wasm=\"/_fundamental/app_bg.wasm\""),
            "{html}"
        );
    }

    #[tokio::test]
    async fn an_installed_shell_wraps_every_page_the_router_serves_and_serves_its_icons() {
        use fundamental_server::{HttpContext, Method, Route, ServerRequest};
        use leptos::prelude::*;

        let router = Router::<()>::new()
            .route(Route::html("/", |ctx: HttpContext<()>| async move {
                Ok(crate::render_page(&ctx, || view! { <main>"home"</main> })
                    .into_document(crate::default_document))
            }))
            .expect("unique route");
        let router = Document::new()
            .icons(&ICONS)
            .install(router)
            .expect("the icon's path is free");
        let page = router
            .dispatch(ServerRequest::new(Method::Get, "/", ()))
            .await;
        let html = String::from_utf8_lossy(page.body()).into_owned();
        assert!(
            html.contains(r#"<link rel="icon" href="/icon.svg""#),
            "{html}"
        );
        let icon = router
            .dispatch(ServerRequest::new(Method::Get, "/icon.svg", ()))
            .await;
        assert_eq!(icon.body(), b"<svg/>");
    }

    #[test]
    fn a_language_is_written_in_the_region_the_site_names() {
        let document = Document::new().locales(vec![Locale::new("en", Some("GB"))]);
        assert_eq!(
            document.regional(&Locale::new("en", None::<&str>)),
            Locale::new("en", Some("GB"))
        );
        assert_eq!(
            document.regional(&Locale::new("de", None::<&str>)),
            Locale::new("de", None::<&str>)
        );
    }
}
