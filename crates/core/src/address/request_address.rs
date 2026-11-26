//! [`RequestAddress`] — what a handler may know about the address it is answering.

#[cfg(not(feature = "std"))]
use alloc::{borrow::ToOwned, string::String, sync::Arc, vec::Vec};
#[cfg(feature = "std")]
use std::sync::Arc;

#[cfg(feature = "std")]
use crate::address::PageLanguage;
use crate::address::{AbsoluteUrl, AddressSpace, LanguagePaths, PagePath};
use crate::i18n::Locale;

/// The address a page is being rendered at, as much of it as a handler may see.
///
/// It deliberately carries no requested path, so a caller-supplied string cannot reach a
/// link or a `Location`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestAddress {
    space: Arc<AddressSpace>,
    locale: Locale,
    prefix: String,
    canonical_path: String,
    language_paths: Option<LanguagePaths>,
    served: bool,
}

impl RequestAddress {
    /// The address for `canonical_path` under `prefix`, resolved against `space`.
    #[must_use]
    pub fn new(
        space: Arc<AddressSpace>,
        locale: Locale,
        prefix: impl Into<String>,
        canonical_path: impl Into<String>,
    ) -> Self {
        Self {
            space,
            locale,
            prefix: prefix.into(),
            canonical_path: canonical_path.into(),
            language_paths: None,
            served: true,
        }
    }

    /// The address of a page whose path differs by language — an entity named by a slug in
    /// each language.
    #[must_use]
    pub fn with_language_paths(mut self, paths: LanguagePaths) -> Self {
        paths
            .path_in(&self.locale)
            .clone_into(&mut self.canonical_path);
        self.language_paths = Some(paths);
        self
    }

    /// Marks this as the address of a page **no route serves** — the not-found page
    /// rendered inside the market it was asked in.
    ///
    /// Its [`canonical_url`](Self::canonical_url) and [`alternate_urls`](Self::alternate_urls)
    /// are `None`, so no search engine is asked to index a 404.
    #[must_use]
    pub fn unserved(mut self) -> Self {
        self.served = false;
        self
    }

    /// Whether a route serves this address; `false` for the not-found page.
    #[must_use]
    pub fn is_served(&self) -> bool {
        self.served
    }

    /// The page's absolute canonical URL, when its address space knows its origin and a
    /// route serves it.
    #[must_use]
    pub fn canonical_url(&self) -> Option<AbsoluteUrl> {
        if !self.served {
            return None;
        }
        if !self.space.language_origins().is_empty() {
            return self.space.url_in(&self.locale, &self.canonical_path);
        }
        self.space.url(&self.prefix, &self.canonical_path)
    }

    /// Every absolute `(hreflang, URL)` alternate for the page, `x-default` included —
    /// when its address space knows its origin, a route serves it, and it is published
    /// in every language (see [`serves_every_page`](Self::serves_every_page)).
    #[must_use]
    pub fn alternate_urls(&self) -> Option<Vec<(String, AbsoluteUrl)>> {
        if !self.served || !self.serves_every_page() {
            return None;
        }
        match &self.language_paths {
            Some(paths) => self.space.alternate_urls_for(paths),
            None => self.space.alternate_urls(&self.canonical_path),
        }
    }

    /// The locale this page is being answered in.
    #[must_use]
    pub fn locale(&self) -> &Locale {
        &self.locale
    }

    /// The leading segment this page is served under, with its slash (`"/nl"`).
    ///
    /// Empty when the page is served without one.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The route template's path — what the page is, independent of language.
    #[must_use]
    pub fn canonical_path(&self) -> &str {
        &self.canonical_path
    }

    /// The address `canonical_path` is served at **in this request's locale**.
    #[must_use]
    pub fn href(&self, canonical_path: &str) -> String {
        if !self.space.language_origins().is_empty() {
            return self.space.address_in(&self.locale, canonical_path);
        }
        self.space.address(&self.prefix, canonical_path)
    }

    /// [`href`](Self::href) for a page a route builder named: its path spelled in this
    /// request's locale, its query and fragment carried as they were built.
    #[must_use]
    pub fn href_to(&self, page: &PagePath) -> String {
        if !self.space.language_origins().is_empty() {
            return self.space.page_address_in(&self.locale, page);
        }
        self.space.page_address(&self.prefix, page)
    }

    /// Every `(hreflang, href)` alternate for the page being rendered.
    #[must_use]
    pub fn alternates(&self) -> Vec<(String, String)> {
        match &self.language_paths {
            Some(paths) => self.space.alternates_for(paths),
            None => self.space.alternates(&self.canonical_path),
        }
    }

    /// The page in each language it is published in, at its address there with `?lang=<tag>`:
    /// what a language switch links to.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn languages(&self) -> Vec<PageLanguage> {
        self.alternates()
            .into_iter()
            .filter_map(|(hreflang, address)| {
                let locale = hreflang.parse::<Locale>().ok()?;
                Some(PageLanguage {
                    href: format!("{address}?lang={locale}"),
                    current: locale == self.locale,
                    locale,
                })
            })
            .collect()
    }

    /// Whether **other pages exist at this address**.
    ///
    /// `false` without a prefix or under a [`PrefixScope::RootOnly`](crate::address::PrefixScope::RootOnly)
    /// one, unless the space has a [default language](AddressSpace::with_default_language).
    #[must_use]
    pub fn serves_every_page(&self) -> bool {
        let bare = self.prefix.trim_start_matches('/');
        if bare.is_empty() {
            return self.space.default_language().is_some()
                || !self.space.language_origins().is_empty();
        }
        self.space
            .prefixes()
            .scope_of(bare)
            .is_some_and(|scope| scope.serves_every_page())
    }

    /// The address space this request was resolved against.
    #[must_use]
    pub fn space(&self) -> &AddressSpace {
        &self.space
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::{LocalePrefixSet, PageSlug, PrefixScope, SlugTable};
    #[cfg(not(feature = "std"))]
    use alloc::string::ToString;

    fn address_at(prefix: &str, canonical_path: &str) -> RequestAddress {
        let prefixes = LocalePrefixSet::new()
            .with("nl", "nl-NL".parse().expect("tag"))
            .expect("nl")
            .with("nl-en", "en-NL".parse().expect("tag"))
            .expect("nl-en")
            .with_scope("pt", "en-PT".parse().expect("tag"), PrefixScope::RootOnly)
            .expect("pt");
        let slugs = SlugTable::builder()
            .page("/why", &[PageSlug::current("nl", "waarom")])
            .page("/trust", &[])
            .build()
            .expect("well formed");
        let space = Arc::new(AddressSpace::new(prefixes, slugs));
        let locale = space
            .prefixes()
            .locale_of(prefix.trim_start_matches('/'))
            .cloned()
            .unwrap_or_else(|| "en".parse().expect("tag"));
        RequestAddress::new(space, locale, prefix, canonical_path)
    }

    #[test]
    fn an_entity_page_names_each_languages_own_path() {
        let paths = LanguagePaths::uniform("/products/eiken-tafel")
            .with("en-NL".parse().expect("tag"), "/products/oak-table");
        let dutch = address_at("/nl", "/products/ignored").with_language_paths(paths.clone());
        assert_eq!(dutch.canonical_path(), "/products/eiken-tafel");
        let alternates = dutch.alternates();
        assert!(
            alternates.contains(&("nl-NL".to_string(), "/nl/products/eiken-tafel".to_string()))
        );
        assert!(
            alternates.contains(&("en-NL".to_string(), "/nl-en/products/oak-table".to_string()))
        );
        let english = address_at("/nl-en", "/products/ignored").with_language_paths(paths);
        assert_eq!(english.canonical_path(), "/products/oak-table");
    }

    #[test]
    fn only_a_prefix_that_serves_the_tree_may_offer_other_pages() {
        assert!(address_at("/nl", "/why").serves_every_page());
        assert!(address_at("/nl-en", "/why").serves_every_page());
        assert!(
            !address_at("/pt", "/").serves_every_page(),
            "a prefix serving one page has no other page to link to — every entry a bar \
             rendered there would resolve straight back to it"
        );
        assert!(
            !address_at("", "/").serves_every_page(),
            "a page outside every locale is in no locale, so nothing is reachable from \
             it by a prefixed link — nothing is site-wide by default"
        );
    }

    #[test]
    fn a_link_is_built_in_the_readers_own_address_space() {
        let address = address_at("/nl", "/why");
        assert_eq!(address.href("/why"), "/nl/waarom");
        assert_eq!(address.href("/trust"), "/nl/trust");
    }

    /// A query or fragment is not a segment.
    #[test]
    fn href_localizes_the_path_and_keeps_its_query() {
        let address = address_at("/nl", "/why");
        assert_eq!(
            address.href("/why?next=%2fa%2Fb#top"),
            "/nl/waarom?next=%2fa%2Fb#top"
        );
        assert_eq!(address.href("/why#top"), "/nl/waarom#top");
        let page = PagePath::of_route("/why")
            .with_query("next", "/a b?c=d&e")
            .with_fragment("top");
        assert_eq!(
            address.href_to(&page),
            "/nl/waarom?next=%2Fa%20b%3Fc%3Dd%26e#top"
        );
        assert_eq!(
            address
                .space()
                .url_in(&"nl-NL".parse().expect("tag"), "/why?x=%2f"),
            None,
            "no origin, no url"
        );
        assert_eq!(
            address
                .space()
                .address_in(&"nl-NL".parse().expect("tag"), "/why?x=%2f"),
            "/nl/waarom?x=%2f"
        );
    }

    /// The path ends in `/` before its query, and the root keeps its one slash.
    #[test]
    fn a_page_spelled_as_a_directory_ends_its_path_in_a_slash() {
        let address = address_at("/nl", "/why");
        let page = PagePath::of_route("/why")
            .with_query("x", "1")
            .with_fragment("top")
            .as_directory();
        assert_eq!(address.href_to(&page), "/nl/waarom/?x=1#top");
        let home = PagePath::of_route("/").as_directory();
        assert_eq!(address.href_to(&home), "/nl/");
        assert_eq!(address_at("", "/").href_to(&home), "/");
        assert!(page.same_page(&PagePath::of_route("/why")));
    }

    #[test]
    fn the_same_link_from_another_market_names_that_market() {
        assert_eq!(address_at("/nl-en", "/why").href("/why"), "/nl-en/why");
    }

    #[test]
    fn alternates_are_derived_from_the_space_rather_than_authored() {
        let alternates = address_at("/nl", "/why").alternates();
        assert!(alternates.contains(&("nl-NL".to_string(), "/nl/waarom".to_string())));
        assert!(alternates.contains(&("en-NL".to_string(), "/nl-en/why".to_string())));
        assert!(alternates.contains(&("x-default".to_string(), "/".to_string())));
    }

    #[test]
    fn an_unprefixed_page_links_to_unprefixed_paths() {
        let address = address_at("", "/");
        assert_eq!(
            address.href("/why"),
            "/why",
            "the chooser at the root has no market to link within"
        );
    }

    #[test]
    fn a_page_can_tell_the_root_from_a_market() {
        assert!(address_at("", "/").prefix().is_empty());
        assert_eq!(address_at("/nl", "/").prefix(), "/nl");
    }

    /// Adding an accessor for the requested path would break `ReturnPath`'s safety.
    #[test]
    fn the_requested_path_is_not_reachable_from_a_request_address() {
        let address = address_at("/nl", "/why");
        assert_eq!(address.canonical_path(), "/why");
        assert_eq!(address.prefix(), "/nl");
        assert_eq!(address.locale().to_string(), "nl-NL");
        assert_eq!(address.href("/why"), "/nl/waarom");
        let rendered = alloc_format(&address);
        assert!(
            !rendered.contains("waarom-acme") && !rendered.contains("requested"),
            "a request address must not carry what was asked for: {rendered}"
        );
    }

    #[test]
    fn a_page_on_a_language_origin_names_its_own_origin_and_links_within_it() {
        let space = AddressSpace::default()
            .with_language_origins(
                [
                    (
                        "nl".parse().expect("tag"),
                        crate::address::WebOrigin::parse("https://winkel.nl").expect("nl"),
                    ),
                    (
                        "en".parse().expect("tag"),
                        crate::address::WebOrigin::parse("https://shop.com").expect("en"),
                    ),
                ],
                &"en".parse().expect("tag"),
            )
            .expect("origins");
        let page = RequestAddress::new(Arc::new(space), "en".parse().expect("tag"), "", "/prijzen");
        assert!(page.serves_every_page());
        assert_eq!(
            page.href("/over"),
            "/over",
            "a link stays on its own origin"
        );
        assert_eq!(
            page.canonical_url().map(|url| url.into_string()),
            Some("https://shop.com/prijzen".to_string())
        );
        assert!(page.alternate_urls().is_some_and(|urls| urls.len() == 3));
    }

    #[test]
    fn a_page_no_route_serves_names_no_url_of_its_own() {
        let origin = crate::address::WebOrigin::parse("https://example.nl").expect("origin");
        let space = Arc::new(
            address_at("/nl", "/why")
                .space()
                .clone()
                .with_origin(origin),
        );
        let served = RequestAddress::new(
            Arc::clone(&space),
            "nl-NL".parse().expect("tag"),
            "/nl",
            "/why",
        );
        assert_eq!(
            served.canonical_url().map(|url| url.into_string()),
            Some("https://example.nl/nl/waarom".to_string())
        );
        assert!(served.alternate_urls().is_some_and(|urls| urls.len() == 3));
        let lost = served.clone().unserved();
        assert!(!lost.is_served());
        assert_eq!(lost.canonical_url(), None);
        assert_eq!(lost.alternate_urls(), None);
        assert_eq!(
            lost.href("/why"),
            "/nl/waarom",
            "it still links within its market"
        );
        let root_only = RequestAddress::new(space, "en-PT".parse().expect("tag"), "/pt", "/");
        assert_eq!(
            root_only.alternate_urls(),
            None,
            "a prefix serving one page publishes no alternates"
        );
    }

    #[cfg(feature = "std")]
    fn alloc_format(address: &RequestAddress) -> String {
        format!("{address:?}")
    }

    #[cfg(not(feature = "std"))]
    fn alloc_format(address: &RequestAddress) -> String {
        alloc::format!("{address:?}")
    }
}
