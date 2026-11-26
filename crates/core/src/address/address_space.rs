//! [`AddressSpace`] — the one canonical address for every page in every locale.

#[cfg(not(feature = "std"))]
use alloc::{
    borrow::ToOwned,
    string::{String, ToString},
    vec::Vec,
};

use crate::address::canonical_spelling::canonical_spelling;
use crate::address::{
    AbsoluteUrl, AddressError, LanguagePaths, LocalePrefixSet, PagePath, PrefixScope,
    ResolvedAddress, SlugTable, WebOrigin,
};
use crate::i18n::Locale;

/// Resolves a requested path to the one canonical address for its page and locale; any
/// other address redirects there permanently.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::{AddressSpace, LocalePrefixSet, PageSlug, PrefixScope, SlugTable};
/// use fundamental_core::i18n::Locale;
///
/// let prefixes = LocalePrefixSet::new()
///     .with("nl", "nl-NL".parse::<Locale>().expect("tag"))?
///     .with_scope("pt", "en-PT".parse::<Locale>().expect("tag"), PrefixScope::RootOnly)?;
/// let slugs = SlugTable::builder()
///     .page("/why", &[PageSlug::current("nl", "waarom")])
///     .build()?;
/// let space = AddressSpace::new(prefixes, slugs);
///
/// // The canonical address serves.
/// let resolved = space.resolve("/nl/waarom", None);
/// assert_eq!(resolved.canonical_path(), "/why");
/// assert_eq!(resolved.redirect_target(), None);
///
/// // The English stem under a Dutch prefix is not that address.
/// assert_eq!(space.resolve("/nl/why", None).redirect_target(), Some("/nl/waarom"));
///
/// // No prefix at all: the page is not resolved to a market, because choosing one
/// // would be a guess about the reader.
/// assert_eq!(space.resolve("/why", None).redirect_target(), Some("/"));
///
/// // A prefix that serves only its root keeps one address.
/// assert_eq!(space.resolve("/pt/anything", None).redirect_target(), Some("/pt"));
/// # Ok::<(), fundamental_core::address::AddressError>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddressSpace {
    prefixes: LocalePrefixSet,
    slugs: SlugTable,
    unaddressed: Vec<String>,
    home: Option<String>,
    unprefixed: Option<Locale>,
    default_language: Option<Locale>,
    origin: Option<WebOrigin>,
    language_origins: Vec<(Locale, WebOrigin)>,
    x_default: Option<Locale>,
    aliases: Vec<(WebOrigin, WebOrigin)>,
}

impl AddressSpace {
    /// An address space over `prefixes` and `slugs`, with `/` unaddressed.
    #[must_use]
    pub fn new(prefixes: LocalePrefixSet, slugs: SlugTable) -> Self {
        Self {
            prefixes,
            slugs,
            unaddressed: Vec::from(["/".to_string()]),
            home: None,
            unprefixed: None,
            default_language: None,
            origin: None,
            language_origins: Vec::new(),
            x_default: None,
            aliases: Vec::new(),
        }
    }

    /// Publishes this space at `origin`, so every address it builds has an absolute URL.
    #[must_use]
    pub fn with_origin(mut self, origin: WebOrigin) -> Self {
        self.origin = Some(origin);
        self
    }

    /// The origin this space is published at, if one was declared.
    #[must_use]
    pub fn origin(&self) -> Option<&WebOrigin> {
        self.origin.as_ref()
    }

    /// Serves each language at an origin of its own: Dutch at `https://winkel.nl`,
    /// English at `https://shop.com`, with no language prefix anywhere.
    ///
    /// # Errors
    /// [`AddressError::LanguageOriginsInvalid`] (`ADDR_011`) when the origins are empty or
    /// repeated, `x_default` has none, or the space already declares a prefix, a default
    /// language, an unprefixed locale, a home or a single origin.
    pub fn with_language_origins<I>(
        mut self,
        origins: I,
        x_default: &Locale,
    ) -> Result<Self, AddressError>
    where
        I: IntoIterator<Item = (Locale, WebOrigin)>,
    {
        let refuse = |reason: &'static str, value: String| {
            Err(AddressError::LanguageOriginsInvalid { reason, value })
        };
        let replaced = [
            (
                self.prefixes.iter().next().is_some(),
                "is not combined with a locale prefix",
            ),
            (
                self.default_language.is_some(),
                "is not combined with a default language",
            ),
            (
                self.unprefixed.is_some(),
                "is not combined with an unprefixed locale",
            ),
            (self.home.is_some(), "is not combined with a home"),
            (
                self.origin.is_some(),
                "is not combined with a single origin",
            ),
        ];
        if let Some((_, reason)) = replaced.iter().find(|(declared, _)| *declared) {
            return refuse(reason, "a language origin".to_string());
        }
        let mut served: Vec<(Locale, WebOrigin)> = Vec::new();
        for (locale, origin) in origins {
            if served.iter().any(|(known, _)| *known == locale) {
                return refuse("is given two origins", locale.to_string());
            }
            if served.iter().any(|(_, known)| *known == origin) {
                return refuse("serves two languages", origin.to_string());
            }
            served.push((locale, origin));
        }
        if served.is_empty() {
            return refuse("names no origin", "a language origin".to_string());
        }
        if !served.iter().any(|(locale, _)| locale == x_default) {
            return refuse("is x-default but has no origin", x_default.to_string());
        }
        self.language_origins = served;
        self.x_default = Some(x_default.clone());
        Ok(self)
    }

    /// Each language and the origin it is served at, when
    /// [`with_language_origins`](Self::with_language_origins) declared them.
    #[must_use]
    pub fn language_origins(&self) -> &[(Locale, WebOrigin)] {
        &self.language_origins
    }

    /// Answers requests addressed to `alias` with a permanent redirect to the same path on
    /// `of`: a second domain for a site, kept only so old links and typed names arrive.
    ///
    /// # Errors
    /// [`AddressError::LanguageOriginsInvalid`] (`ADDR_011`) when `of` is not an origin this
    /// space serves (declare the origins first), or `alias` is one it serves or already
    /// aliases.
    pub fn with_alias(mut self, alias: WebOrigin, of: WebOrigin) -> Result<Self, AddressError> {
        let served = |origin: &WebOrigin| {
            self.origin.as_ref() == Some(origin)
                || self
                    .language_origins
                    .iter()
                    .any(|(_, known)| known == origin)
        };
        if !served(&of) {
            return Err(AddressError::LanguageOriginsInvalid {
                reason: "is an alias of an origin this space does not serve",
                value: of.to_string(),
            });
        }
        if served(&alias) || self.aliases.iter().any(|(known, _)| *known == alias) {
            return Err(AddressError::LanguageOriginsInvalid {
                reason: "is already served or aliased",
                value: alias.to_string(),
            });
        }
        self.aliases.push((alias, of));
        Ok(self)
    }

    /// The origin a request addressed to `authority` is redirected to, when it names an alias.
    #[must_use]
    pub fn alias_target(&self, authority: &str) -> Option<&WebOrigin> {
        self.aliases
            .iter()
            .find(|(alias, _)| alias.serves_authority(authority))
            .map(|(_, of)| of)
    }

    /// The language a request addressed to `authority` is answered in, when languages have
    /// origins: the one whose origin it names, otherwise `x-default`'s.
    #[must_use]
    pub fn language_of_authority(&self, authority: Option<&str>) -> Option<&Locale> {
        authority
            .and_then(|authority| {
                self.language_origins
                    .iter()
                    .find(|(_, origin)| origin.serves_authority(authority))
                    .map(|(locale, _)| locale)
            })
            .or(self.x_default.as_ref())
    }

    /// Resolves `requested_path` addressed to `authority`.
    #[must_use]
    pub fn resolve_on(
        &self,
        authority: Option<&str>,
        requested_path: &str,
        fallback: Option<&Locale>,
    ) -> ResolvedAddress {
        let Some(locale) = self.language_of_authority(authority) else {
            return self.resolve(requested_path, fallback);
        };
        let requested = normalise(requested_path);
        let spelled = canonical_spelling(requested);
        let language = locale.language();
        let canonical_path = self.slugs.de_alias(&spelled, language).into_owned();
        let canonical_address = self.slugs.localize(&canonical_path, language).into_owned();
        let redirect = (requested != canonical_address).then(|| canonical_address.clone());
        ResolvedAddress::prefixed(
            String::new(),
            locale.clone(),
            canonical_path,
            canonical_address,
            redirect,
        )
    }

    /// The address `canonical_path` is served at in `locale`, without a request: what a
    /// mail, another surface's page or a job links to.
    #[must_use]
    pub fn address_in(&self, locale: &Locale, canonical_path: &str) -> String {
        let (path, suffix) = split_link(canonical_path);
        let mut address = self.path_in(locale, path);
        address.push_str(suffix);
        address
    }

    /// [`address_in`](Self::address_in) for a path with no query or fragment.
    fn path_in(&self, locale: &Locale, canonical_path: &str) -> String {
        let canonical_path = canonical_spelling(canonical_path);
        if self.language_origins.is_empty() {
            let prefix = self
                .prefixes
                .iter()
                .find(|(_, served, scope)| *served == locale && scope.serves_every_page())
                .map(|(prefix, _, _)| prefix);
            if let Some(prefix) = prefix {
                return self.address(prefix, &canonical_path);
            }
            if self.default_language.as_ref() == Some(locale) {
                return self.address("", &canonical_path);
            }
        }
        self.slugs
            .localize(&canonical_path, locale.language())
            .into_owned()
    }

    /// The absolute URL of [`address_in`](Self::address_in): on that language's origin where
    /// languages have origins, otherwise on the space's one origin.
    #[must_use]
    pub fn url_in(&self, locale: &Locale, canonical_path: &str) -> Option<AbsoluteUrl> {
        let origin = self.origin_of(locale).or(self.origin.as_ref())?;
        Some(origin.url(&self.address_in(locale, canonical_path)))
    }

    /// The one language every address of this space is spelled in, if there is one.
    #[must_use]
    pub fn path_language(&self) -> Option<&str> {
        let mut languages = self
            .language_origins
            .iter()
            .map(|(locale, _)| locale.language())
            .chain(self.default_language.iter().map(Locale::language))
            .chain(
                self.prefixes
                    .iter()
                    .filter(|(_, _, scope)| scope.serves_every_page())
                    .filter_map(|(prefix, _, _)| self.prefixes.path_language_of(prefix)),
            );
        let first = languages.next()?;
        if languages.any(|language| language != first) {
            return None;
        }
        let unaddressed_page = self.default_language.is_none()
            && self.language_origins.is_empty()
            && self.unaddressed.iter().any(|path| path != "/");
        (!unaddressed_page).then_some(first)
    }

    fn origin_of(&self, locale: &Locale) -> Option<&WebOrigin> {
        self.language_origins
            .iter()
            .find(|(known, _)| known == locale)
            .map(|(_, origin)| origin)
    }

    /// Serves every unprefixed path as a page in `locale`.
    ///
    /// # Errors
    /// [`AddressError::UnprefixedLocaleIsPrefixed`] (`ADDR_007`) when a prefix already
    /// serves `locale`; [`AddressError::DefaultLanguageConflicts`] (`ADDR_010`) when the
    /// space already declares [`with_home`](Self::with_home) or
    /// [`with_unprefixed_locale`](Self::with_unprefixed_locale), which this replaces, or when
    /// languages already have [origins](Self::with_language_origins).
    pub fn with_default_language(mut self, locale: Locale) -> Result<Self, AddressError> {
        if let Some((prefix, _, _)) = self
            .prefixes
            .iter()
            .find(|(_, served, _)| **served == locale)
        {
            return Err(AddressError::UnprefixedLocaleIsPrefixed {
                locale: locale.to_string(),
                prefix: prefix.to_owned(),
            });
        }
        let conflict = match (&self.home, &self.unprefixed) {
            (Some(_), _) => Some("a home the unprefixed world is sent to"),
            (None, Some(_)) => Some("a language for unaddressed paths"),
            (None, None) if !self.language_origins.is_empty() => Some("an origin per language"),
            (None, None) => None,
        };
        if let Some(with) = conflict {
            return Err(AddressError::DefaultLanguageConflicts {
                locale: locale.to_string(),
                with,
            });
        }
        self.default_language = Some(locale);
        Ok(self)
    }

    /// The language unprefixed pages are served in, when
    /// [`with_default_language`](Self::with_default_language) declared one.
    #[must_use]
    pub fn default_language(&self) -> Option<&Locale> {
        self.default_language.as_ref()
    }

    /// Sends the unprefixed world to `home` instead of to `/`.
    ///
    /// Call it together with [`with_unaddressed`](Self::with_unaddressed)`([])`, or `/` stays
    /// served in place and nothing redirects.
    #[must_use]
    pub fn with_home(mut self, home: impl Into<String>) -> Self {
        self.home = Some(home.into());
        self
    }

    /// Replaces the set of paths that are served without a prefix.
    #[must_use]
    pub fn with_unaddressed<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.unaddressed = paths.into_iter().map(Into::into).collect();
        self
    }

    /// Declares the language the unaddressed paths are served in.
    ///
    /// # Errors
    /// [`AddressError::UnprefixedLocaleIsPrefixed`] (`ADDR_007`) when a prefix already serves
    /// `locale`, [`AddressError::DefaultLanguageConflicts`] (`ADDR_010`) when a default language
    /// is declared, [`AddressError::LanguageOriginsInvalid`] (`ADDR_011`) when languages have
    /// origins.
    pub fn with_unprefixed_locale(mut self, locale: Locale) -> Result<Self, AddressError> {
        if !self.language_origins.is_empty() {
            return Err(AddressError::LanguageOriginsInvalid {
                reason: "is not combined with an unprefixed locale",
                value: locale.to_string(),
            });
        }
        if let Some(default) = &self.default_language {
            return Err(AddressError::DefaultLanguageConflicts {
                locale: default.to_string(),
                with: "a language for unaddressed paths",
            });
        }
        if let Some((prefix, _, _)) = self
            .prefixes
            .iter()
            .find(|(_, served, _)| **served == locale)
        {
            return Err(AddressError::UnprefixedLocaleIsPrefixed {
                locale: locale.to_string(),
                prefix: prefix.to_owned(),
            });
        }
        self.unprefixed = Some(locale);
        Ok(self)
    }

    /// The prefixes this space answers.
    #[must_use]
    pub fn prefixes(&self) -> &LocalePrefixSet {
        &self.prefixes
    }

    /// The per-language slugs this space translates paths through.
    #[must_use]
    pub fn slugs(&self) -> &SlugTable {
        &self.slugs
    }

    /// Resolves `requested_path`, falling back to `fallback` when no prefix decided
    /// the locale.
    #[must_use]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    pub fn resolve(&self, requested_path: &str, fallback: Option<&Locale>) -> ResolvedAddress {
        let requested = normalise(requested_path);
        // Resolved in its one spelling, compared as asked, so another spelling redirects.
        let spelled = canonical_spelling(requested);

        let Some(matched) = self.prefixes.resolve(&spelled) else {
            if let Some(locale) = &self.default_language {
                let language = locale.language();
                let canonical_path = self.slugs.de_alias(&spelled, language).into_owned();
                let canonical_address = self.slugs.localize(&canonical_path, language).into_owned();
                let redirect = (requested != canonical_address).then(|| canonical_address.clone());
                return ResolvedAddress::prefixed(
                    String::new(),
                    locale.clone(),
                    canonical_path,
                    canonical_address,
                    redirect,
                );
            }
            // Rule 1: no prefix, no page; the router decides once it sees what matched.
            if !self.is_unaddressed(requested) {
                let redirect = Some(self.home().to_owned());
                return ResolvedAddress::unprefixed(
                    requested.to_owned(),
                    fallback.cloned(),
                    redirect,
                );
            }
            return match &self.unprefixed {
                Some(locale) => {
                    ResolvedAddress::unprefixed_in(requested.to_owned(), locale.clone())
                }
                None => ResolvedAddress::unprefixed(requested.to_owned(), fallback.cloned(), None),
            };
        };

        let (prefix, locale, rest) = (matched.prefix(), matched.locale(), matched.rest());
        // The prefix's path language, not the reader's: one country shares one vocabulary.
        let language = matched.path_language().to_owned();
        let prefix_path = {
            let mut path = String::with_capacity(prefix.len() + 1);
            path.push('/');
            path.push_str(prefix);
            path
        };

        // A prefix that serves only its root has exactly one address, so every
        // deeper path is a second name for it.
        if matched.scope() == PrefixScope::RootOnly {
            let canonical_address = prefix_path.clone();
            let redirect = (requested != canonical_address).then(|| canonical_address.clone());
            return ResolvedAddress::prefixed(
                prefix_path,
                locale.clone(),
                "/".to_string(),
                canonical_address,
                redirect,
            );
        }

        let canonical_path = self.slugs.de_alias(rest, &language).into_owned();
        let localized = self.slugs.localize(&canonical_path, &language);
        let canonical_address = join(&prefix_path, &localized);
        let redirect = (requested != canonical_address).then(|| canonical_address.clone());

        ResolvedAddress::prefixed(
            prefix_path,
            locale.clone(),
            canonical_path,
            canonical_address,
            redirect,
        )
    }

    /// The address `canonical_path` is served at under `prefix`.
    #[must_use]
    pub fn address(&self, prefix: &str, canonical_path: &str) -> String {
        let (path, suffix) = split_link(canonical_path);
        let mut address = self.path_under(prefix, path);
        address.push_str(suffix);
        address
    }

    /// [`address`](Self::address) for a path with no query or fragment.
    fn path_under(&self, prefix: &str, canonical_path: &str) -> String {
        let bare = prefix.trim_start_matches('/');
        if bare.is_empty()
            && let Some(locale) = &self.default_language
        {
            return self
                .slugs
                .localize(canonical_path, locale.language())
                .into_owned();
        }
        let Some(language) = self.prefixes.path_language_of(bare) else {
            return canonical_path.to_owned();
        };
        let mut prefix_path = String::with_capacity(prefix.len() + 1);
        if !prefix.starts_with('/') {
            prefix_path.push('/');
        }
        prefix_path.push_str(prefix);

        // A root-only prefix has one address; `/pt/why` would be a link that redirects.
        if self.prefixes.scope_of(bare) == Some(PrefixScope::RootOnly) {
            return prefix_path;
        }

        join(&prefix_path, &self.slugs.localize(canonical_path, language))
    }

    /// Every `(hreflang, href)` alternate for `canonical_path`, plus `x-default`.
    #[must_use]
    pub fn alternates(&self, canonical_path: &str) -> Vec<(String, String)> {
        self.alternates_for(&LanguagePaths::uniform(canonical_path))
    }

    /// [`alternates`](Self::alternates) for a page spelled differently per language: each
    /// language's alternate is built from its own path in `paths`.
    #[must_use]
    pub fn alternates_for(&self, paths: &LanguagePaths) -> Vec<(String, String)> {
        if !self.language_origins.is_empty() {
            return self
                .language_alternates(paths)
                .into_iter()
                .map(|(hreflang, url)| (hreflang, url.into_string()))
                .collect();
        }
        let mut alternates: Vec<(String, String)> = self
            .prefixes
            .iter()
            .filter(|(_, _, scope)| scope.serves_every_page())
            .map(|(prefix, locale, _)| {
                (
                    locale.to_string(),
                    self.address(prefix, paths.path_in(locale)),
                )
            })
            .collect();
        if let Some(locale) = &self.default_language {
            let unprefixed = self.address("", paths.path_in(locale));
            alternates.insert(0, (locale.to_string(), unprefixed.clone()));
            alternates.push(("x-default".to_string(), unprefixed));
            return alternates;
        }
        if let Some(locale) = &self.unprefixed {
            let unprefixed = normalise(paths.path_in(locale));
            if self.is_unaddressed(unprefixed) {
                alternates.push((locale.to_string(), unprefixed.to_owned()));
            }
        }
        alternates.push(("x-default".to_string(), self.home().to_owned()));
        alternates
    }

    /// The absolute URL of `canonical_path` under `prefix`, when this space knows its
    /// [origin](Self::with_origin).
    #[must_use]
    pub fn url(&self, prefix: &str, canonical_path: &str) -> Option<AbsoluteUrl> {
        let origin = self.origin.as_ref()?;
        let (path, suffix) = split_link(canonical_path);
        let mut address = self.path_under(prefix, &canonical_spelling(path));
        address.push_str(suffix);
        Some(origin.url(&address))
    }

    /// [`address`](Self::address) for a page a route builder named, its query and fragment
    /// after the spelled path.
    #[must_use]
    pub fn page_address(&self, prefix: &str, page: &PagePath) -> String {
        page.completed(self.path_under(prefix, page.route()))
    }

    /// [`address_in`](Self::address_in) for a page a route builder named.
    #[must_use]
    pub fn page_address_in(&self, locale: &Locale, page: &PagePath) -> String {
        page.completed(self.path_in(locale, page.route()))
    }

    /// [`url`](Self::url) for a page a route builder named.
    #[must_use]
    pub fn page_url(&self, prefix: &str, page: &PagePath) -> Option<AbsoluteUrl> {
        let origin = self.origin.as_ref()?;
        Some(origin.url(&self.page_address(prefix, page)))
    }

    /// [`url_in`](Self::url_in) for a page a route builder named.
    #[must_use]
    pub fn page_url_in(&self, locale: &Locale, page: &PagePath) -> Option<AbsoluteUrl> {
        let origin = self.origin_of(locale).or(self.origin.as_ref())?;
        Some(origin.url(&self.page_address_in(locale, page)))
    }

    /// Every `(hreflang, absolute URL)` alternate for `canonical_path`, `x-default`
    /// included, when this space knows its [origin](Self::with_origin).
    #[must_use]
    pub fn alternate_urls(&self, canonical_path: &str) -> Option<Vec<(String, AbsoluteUrl)>> {
        self.alternate_urls_for(&LanguagePaths::uniform(canonical_path))
    }

    /// [`alternate_urls`](Self::alternate_urls) for a page spelled differently per language.
    #[must_use]
    pub fn alternate_urls_for(&self, paths: &LanguagePaths) -> Option<Vec<(String, AbsoluteUrl)>> {
        if !self.language_origins.is_empty() {
            return Some(self.language_alternates(paths));
        }
        let origin = self.origin.as_ref()?;
        let mut spelled = LanguagePaths::uniform(canonical_spelling(paths.default_path()));
        for (_, locale, _) in self.prefixes.iter() {
            spelled = spelled.with(locale.clone(), canonical_spelling(paths.path_in(locale)));
        }
        for locale in self.default_language.iter().chain(self.unprefixed.iter()) {
            spelled = spelled.with(locale.clone(), canonical_spelling(paths.path_in(locale)));
        }
        Some(
            self.alternates_for(&spelled)
                .into_iter()
                .map(|(hreflang, address)| (hreflang, origin.url(&address)))
                .collect(),
        )
    }

    /// Every language's URL for its path in `paths` on its own origin, and `x-default`.
    fn language_alternates(&self, paths: &LanguagePaths) -> Vec<(String, AbsoluteUrl)> {
        let mut alternates: Vec<(String, AbsoluteUrl)> = self
            .language_origins
            .iter()
            .map(|(locale, origin)| {
                (
                    locale.to_string(),
                    origin.url(&self.address_in(locale, paths.path_in(locale))),
                )
            })
            .collect();
        if let Some(x_default) = &self.x_default
            && let Some(origin) = self.origin_of(x_default)
        {
            alternates.push((
                "x-default".to_string(),
                origin.url(&self.address_in(x_default, paths.path_in(x_default))),
            ));
        }
        alternates
    }

    /// The address the unprefixed world is sent to.
    #[must_use]
    pub fn home(&self) -> &str {
        self.home
            .as_deref()
            .or_else(|| self.unaddressed.first().map(String::as_str))
            .unwrap_or("/")
    }

    fn is_unaddressed(&self, path: &str) -> bool {
        self.unaddressed.iter().any(|known| known == path)
    }
}

/// Strips a trailing slash so `/nl/` and `/nl` are one address, keeping root as `/`.
fn normalise(path: &str) -> &str {
    match path.len() > 1 && path.ends_with('/') {
        true => path.trim_end_matches('/'),
        false => path,
    }
}

/// Joins a prefix with a path, keeping the prefix's own root free of a trailing `/`.
fn join(prefix: &str, path: &str) -> String {
    if path == "/" {
        return prefix.to_owned();
    }
    let mut joined = String::with_capacity(prefix.len() + path.len());
    joined.push_str(prefix);
    joined.push_str(path);
    joined
}

/// `link` split into its path and the `?query#fragment` carried byte for byte.
fn split_link(link: &str) -> (&str, &str) {
    link.find(['?', '#'])
        .map_or((link, ""), |at| link.split_at(at))
}

#[cfg(test)]
mod default_language {
    //! The default language at the bare domain, the others under a prefix.

    #[cfg(not(feature = "std"))]
    use alloc::{string::ToString, vec::Vec};

    use super::AddressSpace;
    use crate::address::{LocalePrefixSet, SlugTable, WebOrigin};
    use crate::i18n::Locale;

    fn nl() -> Locale {
        "nl".parse().expect("tag")
    }

    fn space() -> AddressSpace {
        let prefixes = LocalePrefixSet::new()
            .with("en", "en".parse::<Locale>().expect("tag"))
            .expect("en");
        let slugs = SlugTable::builder()
            .page("/", &[])
            .page("/prijzen", &[])
            .build()
            .expect("a table");
        AddressSpace::new(prefixes, slugs)
            .with_default_language(nl())
            .expect("nl is not prefixed")
    }

    #[test]
    fn an_unprefixed_page_is_served_in_the_default_language() {
        for path in ["/prijzen", "/"] {
            let resolved = space().resolve(path, Some(&"en".parse().expect("tag")));
            assert_eq!(resolved.canonical_path(), path);
            assert_eq!(resolved.redirect_target(), None, "{path}");
            assert_eq!(resolved.locale(), Some(&nl()), "{path}");
            assert!(
                resolved.locale_from_path(),
                "the address decided the language"
            );
        }
    }

    #[test]
    fn the_default_language_is_advertised_for_every_page_and_names_x_default() {
        let alternates = space().alternates("/prijzen");
        assert_eq!(
            alternates,
            [
                ("nl".to_string(), "/prijzen".to_string()),
                ("en".to_string(), "/en/prijzen".to_string()),
                ("x-default".to_string(), "/prijzen".to_string()),
            ]
        );
        assert_eq!(space().address("", "/prijzen"), "/prijzen");
    }

    #[test]
    fn a_default_language_refuses_what_it_replaces() {
        let prefixes = || {
            LocalePrefixSet::new()
                .with("en", "en".parse::<Locale>().expect("tag"))
                .expect("en")
        };
        let with_home = AddressSpace::new(prefixes(), SlugTable::default())
            .with_home("/en")
            .with_default_language(nl())
            .expect_err("home conflicts");
        assert_eq!(with_home.code(), "ADDR_010");
        let with_unprefixed = AddressSpace::new(prefixes(), SlugTable::default())
            .with_unprefixed_locale(nl())
            .expect("unprefixed")
            .with_default_language(nl())
            .expect_err("unprefixed locale conflicts");
        assert_eq!(with_unprefixed.code(), "ADDR_010");
        let after = space()
            .with_unprefixed_locale(nl())
            .expect_err("the other order is refused too");
        assert_eq!(after.code(), "ADDR_010");
        let prefixed = AddressSpace::new(prefixes(), SlugTable::default())
            .with_default_language("en".parse().expect("tag"))
            .expect_err("en is prefixed");
        assert_eq!(prefixed.code(), "ADDR_007");
    }

    #[test]
    fn a_second_spelling_of_a_page_redirects_to_its_canonical() {
        let cases = [
            ("/en//prijzen", "/en/prijzen"),
            ("//prijzen", "/prijzen"),
            ("//en/prijzen", "/en/prijzen"),
            ("/en/prijzen/a%2fb", "/en/prijzen/a%2Fb"),
        ];
        for (asked, canonical) in cases {
            let resolved = space().resolve(asked, None);
            assert_eq!(resolved.redirect_target(), Some(canonical), "{asked}");
        }
        assert_eq!(
            space().resolve("/en/prijzen/a%2Fb", None).redirect_target(),
            None
        );
    }

    #[test]
    fn an_absolute_url_is_the_origin_and_the_one_address() {
        let origin = WebOrigin::parse("https://winkel.nl").expect("origin");
        let published = space().with_origin(origin);
        assert_eq!(
            published
                .url("/en", "/prijzen")
                .map(|url| url.into_string()),
            Some("https://winkel.nl/en/prijzen".to_string())
        );
        assert_eq!(
            published.url("", "//prijzen").map(|url| url.into_string()),
            Some("https://winkel.nl/prijzen".to_string()),
            "a doubled slash cannot publish a second URL, nor name another host"
        );
        let alternates: Vec<(String, String)> = published
            .alternate_urls("/prijzen")
            .expect("an origin")
            .into_iter()
            .map(|(hreflang, url)| (hreflang, url.into_string()))
            .collect();
        assert!(
            alternates.contains(&("en".to_string(), "https://winkel.nl/en/prijzen".to_string()))
        );
        assert!(alternates.contains(&(
            "x-default".to_string(),
            "https://winkel.nl/prijzen".to_string()
        )));
        assert_eq!(
            space().url("", "/prijzen"),
            None,
            "no origin, no absolute URL"
        );
    }
}

#[cfg(test)]
mod language_of_record {
    //! Every address spelled in the site's language of record under every prefix.

    #[cfg(not(feature = "std"))]
    use alloc::string::{String, ToString};

    use super::AddressSpace;
    use crate::address::{LocalePrefixSet, PageSlug, PrefixScope, SlugTable, WebOrigin};
    use crate::i18n::Locale;

    fn tag(tag: &str) -> Locale {
        tag.parse().expect("tag")
    }

    fn slugs() -> SlugTable {
        SlugTable::builder()
            .page("/", &[])
            .page("/aanmelden", &[PageSlug::current("en", "sign-in")])
            .page("/{team}", &[])
            .page(
                "/{team}/aanvragen",
                &[PageSlug::current("en", "build-requests")],
            )
            .build()
            .expect("a table")
    }

    /// A site rooted in `root`, its other language under a prefix.
    fn site(root: &str, other: &str) -> AddressSpace {
        let prefixes = LocalePrefixSet::new()
            .with_paths_in(other, tag(other), PrefixScope::Full, root)
            .expect("prefix");
        AddressSpace::new(prefixes, slugs())
            .with_default_language(tag(root))
            .expect("root is not prefixed")
    }

    #[test]
    fn an_english_rooted_site_spells_every_prefix_in_english() {
        let english_site = site("en", "nl");
        for (asked, canonical) in [
            ("/sign-in", "/aanmelden"),
            ("/nl/sign-in", "/aanmelden"),
            ("/nl/acme/build-requests", "/acme/aanvragen"),
        ] {
            let resolved = english_site.resolve(asked, None);
            assert_eq!(resolved.canonical_path(), canonical, "{asked}");
            assert_eq!(resolved.redirect_target(), None, "{asked}");
        }
        assert_eq!(
            english_site
                .resolve("/nl/aanmelden", None)
                .redirect_target(),
            Some("/nl/sign-in"),
            "a Dutch spelling answers the canonical-spelling redirect"
        );
        assert_eq!(
            english_site.address("nl", "/acme/aanvragen"),
            "/nl/acme/build-requests"
        );
        assert_eq!(english_site.address("", "/aanmelden"), "/sign-in");
    }

    #[test]
    fn a_dutch_rooted_site_keeps_its_dutch_addresses_under_every_prefix() {
        let dutch_site = site("nl", "en");
        assert_eq!(dutch_site.address("en", "/aanmelden"), "/en/aanmelden");
        assert_eq!(dutch_site.address("", "/acme/aanvragen"), "/acme/aanvragen");
        let resolved = dutch_site.resolve("/en/aanmelden", None);
        assert_eq!(resolved.canonical_path(), "/aanmelden");
        assert_eq!(resolved.redirect_target(), None);
    }

    #[test]
    fn one_path_language_is_named_and_a_mix_is_not() {
        assert_eq!(site("en", "nl").path_language(), Some("en"));
        assert_eq!(site("nl", "en").path_language(), Some("nl"));

        let mixed = AddressSpace::new(
            LocalePrefixSet::new().with("en", tag("en")).expect("en"),
            slugs(),
        )
        .with_default_language(tag("nl"))
        .expect("nl");
        assert_eq!(
            mixed.path_language(),
            None,
            "Dutch at the root, English under /en"
        );

        let root_only_elsewhere = AddressSpace::new(
            LocalePrefixSet::new()
                .with_paths_in("en", tag("en"), PrefixScope::Full, "nl")
                .expect("en")
                .with_scope("de", tag("de"), PrefixScope::RootOnly)
                .expect("de"),
            slugs(),
        )
        .with_default_language(tag("nl"))
        .expect("nl");
        assert_eq!(
            root_only_elsewhere.path_language(),
            Some("nl"),
            "a prefix serving only its root spells no address"
        );
    }

    #[test]
    fn an_unprefixed_page_or_an_origin_per_language_has_no_one_path_language() {
        let unaddressed = AddressSpace::new(
            LocalePrefixSet::new().with("nl", tag("nl")).expect("nl"),
            slugs(),
        )
        .with_unaddressed(["/", "/aanmelden"]);
        assert_eq!(
            unaddressed.path_language(),
            None,
            "an unprefixed page is served in no declared spelling"
        );
        assert_eq!(
            AddressSpace::new(
                LocalePrefixSet::new().with("nl", tag("nl")).expect("nl"),
                slugs()
            )
            .path_language(),
            Some("nl"),
            "only `/` unprefixed, which has no segment to spell"
        );

        let origins = |second: &str| {
            AddressSpace::new(LocalePrefixSet::new(), slugs())
                .with_language_origins(
                    [
                        (tag("nl"), WebOrigin::parse("https://winkel.nl").expect("o")),
                        (
                            tag(second),
                            WebOrigin::parse("https://shop.com").expect("o"),
                        ),
                    ],
                    &tag("nl"),
                )
                .expect("origins")
        };
        assert_eq!(
            origins("en").path_language(),
            None,
            "each origin spells its own"
        );
        assert_eq!(AddressSpace::default().path_language(), None);
    }

    #[test]
    fn an_address_without_a_request_follows_the_locale_prefix() {
        let english_site = site("en", "nl");
        assert_eq!(
            english_site.address_in(&tag("nl"), "/aanmelden"),
            "/nl/sign-in"
        );
        assert_eq!(
            english_site.address_in(&tag("en"), "/aanmelden"),
            "/sign-in"
        );
        let dutch_site = site("nl", "en");
        assert_eq!(
            dutch_site.address_in(&tag("en"), "/aanmelden"),
            "/en/aanmelden"
        );
        assert_eq!(
            dutch_site.address_in(&tag("nl"), "//aanmelden"),
            "/aanmelden"
        );

        let published =
            english_site.with_origin(WebOrigin::parse("https://my.shop.com").expect("o"));
        assert_eq!(
            published
                .url_in(&tag("nl"), "/acme/aanvragen")
                .map(|url| url.into_string()),
            Some("https://my.shop.com/nl/acme/build-requests".to_string())
        );
        assert_eq!(
            published
                .url_in(&tag("en"), "/aanmelden")
                .map(|url| url.into_string()),
            Some(String::from("https://my.shop.com/sign-in"))
        );
    }
}

#[cfg(test)]
mod language_origins {
    //! One brand in two languages on two origins, with two alias domains.

    #[cfg(not(feature = "std"))]
    use alloc::{string::ToString, vec::Vec};

    use super::AddressSpace;
    use crate::address::{LocalePrefixSet, SlugTable, WebOrigin};
    use crate::i18n::Locale;

    fn tag(tag: &str) -> Locale {
        tag.parse().expect("tag")
    }

    fn origin(value: &str) -> WebOrigin {
        WebOrigin::parse(value).expect("origin")
    }

    fn space() -> AddressSpace {
        AddressSpace::new(LocalePrefixSet::new(), SlugTable::default())
            .with_language_origins(
                [
                    (tag("nl"), origin("https://winkel.nl")),
                    (tag("en"), origin("https://shop.com")),
                ],
                &tag("en"),
            )
            .expect("two origins")
            .with_alias(origin("https://winkel.com"), origin("https://winkel.nl"))
            .expect("an alias")
            .with_alias(origin("https://shop.nl"), origin("https://shop.com"))
            .expect("an alias")
    }

    #[test]
    fn the_authority_chooses_the_language_and_an_unknown_one_is_x_default() {
        let space = space();
        let dutch = space.resolve_on(Some("winkel.nl"), "/prijzen", Some(&tag("en")));
        assert_eq!(dutch.locale(), Some(&tag("nl")));
        assert_eq!(dutch.canonical_path(), "/prijzen");
        assert_eq!(dutch.redirect_target(), None);
        assert!(dutch.locale_from_path());
        let english = space.resolve_on(Some("SHOP.com:443"), "/prijzen", None);
        assert_eq!(english.locale(), Some(&tag("en")));
        let forged = space.resolve_on(Some("evil.example"), "/prijzen", Some(&tag("nl")));
        assert_eq!(
            forged.locale(),
            Some(&tag("en")),
            "an unknown host is x-default's"
        );
        assert_eq!(
            space
                .resolve_on(Some("winkel.nl"), "//prijzen", None)
                .redirect_target(),
            Some("/prijzen")
        );
    }

    #[test]
    fn every_page_names_the_same_path_on_each_origin_and_x_default_on_english() {
        let alternates: Vec<(String, String)> = space()
            .alternate_urls("/prijzen")
            .expect("origins")
            .into_iter()
            .map(|(hreflang, url)| (hreflang, url.into_string()))
            .collect();
        assert_eq!(
            alternates,
            [
                ("nl".to_string(), "https://winkel.nl/prijzen".to_string()),
                ("en".to_string(), "https://shop.com/prijzen".to_string()),
                (
                    "x-default".to_string(),
                    "https://shop.com/prijzen".to_string()
                ),
            ]
        );
        assert_eq!(
            space().alternates("/prijzen")[1].1,
            "https://shop.com/prijzen",
            "a switch built from alternates links to the other site"
        );
        assert_eq!(
            space()
                .url_in(&tag("nl"), "/prijzen")
                .map(|url| url.into_string()),
            Some("https://winkel.nl/prijzen".to_string())
        );
    }

    #[test]
    fn an_alias_names_its_site_and_nothing_else() {
        let space = space();
        assert_eq!(
            space.alias_target("winkel.com").map(WebOrigin::as_str),
            Some("https://winkel.nl")
        );
        assert_eq!(
            space.alias_target("shop.nl:443").map(WebOrigin::as_str),
            Some("https://shop.com")
        );
        assert_eq!(space.alias_target("winkel.nl"), None);
        assert_eq!(space.alias_target("evil.example"), None);
    }

    #[test]
    fn an_inconsistent_setup_is_addr_011() {
        let two = || {
            [
                (tag("nl"), origin("https://winkel.nl")),
                (tag("en"), origin("https://shop.com")),
            ]
        };
        let prefixed = AddressSpace::new(
            LocalePrefixSet::new().with("en", tag("en")).expect("en"),
            SlugTable::default(),
        )
        .with_language_origins(two(), &tag("en"))
        .expect_err("prefix and origins");
        assert_eq!(prefixed.code(), "ADDR_011");
        let twice = AddressSpace::default()
            .with_language_origins(
                [
                    (tag("nl"), origin("https://winkel.nl")),
                    (tag("nl"), origin("https://shop.com")),
                ],
                &tag("nl"),
            )
            .expect_err("a language twice");
        assert_eq!(twice.code(), "ADDR_011");
        let unserved_default = AddressSpace::default()
            .with_language_origins(two(), &tag("de"))
            .expect_err("x-default without an origin");
        assert_eq!(unserved_default.code(), "ADDR_011");
        let foreign_alias = space()
            .with_alias(origin("https://acme.nl"), origin("https://example.nl"))
            .expect_err("an alias of nothing served");
        assert_eq!(foreign_alias.code(), "ADDR_011");
        let then_default = space()
            .with_default_language(tag("nl"))
            .expect_err("a default beside origins");
        assert_eq!(then_default.code(), "ADDR_010");
    }
}

#[cfg(test)]
mod default_market {
    //! A surface with no chooser: the unprefixed world resolves into a market at dispatch.

    use super::AddressSpace;
    use crate::address::{LocalePrefixSet, PrefixScope, SlugTable};
    use crate::i18n::Locale;

    fn space() -> AddressSpace {
        let prefixes = LocalePrefixSet::new()
            .with_scope(
                "nl",
                "nl-NL".parse::<Locale>().expect("tag"),
                PrefixScope::Full,
            )
            .expect("one prefix");
        let slugs = SlugTable::builder()
            .page("/", &[])
            .page("/login", &[])
            .build()
            .expect("a table");
        AddressSpace::new(prefixes, slugs)
            .with_unaddressed::<[&str; 0], &str>([])
            .with_home("/nl")
    }

    /// `/` is gated, the gate sends to `/login`, and rule 1 would send `/login` back to `/`.
    #[test]
    fn the_bare_domain_resolves_into_a_market_rather_than_back_to_itself() {
        let space = space();
        assert_eq!(space.resolve("/", None).redirect_target(), Some("/nl"));
        assert_eq!(space.resolve("/login", None).redirect_target(), Some("/nl"));
        assert_eq!(
            space.resolve("/nl/login", None).redirect_target(),
            None,
            "the address inside the market answers in place, or the redirect never ends"
        );
    }

    #[test]
    fn without_a_default_the_unprefixed_root_points_at_itself() {
        let looping = AddressSpace::new(LocalePrefixSet::new(), SlugTable::default())
            .with_unaddressed::<[&str; 0], &str>([]);
        assert_eq!(looping.home(), "/");
        assert_eq!(looping.resolve("/", None).redirect_target(), Some("/"));
    }

    /// `x-default` names the unprefixed entry point, so it follows the default too.
    #[test]
    fn the_crawlers_default_entry_is_the_one_a_reader_gets() {
        assert_eq!(space().home(), "/nl");
    }
}

#[cfg(test)]
mod unprefixed_default {
    //! Dutch at `/`, English under `/en`.

    #[cfg(not(feature = "std"))]
    use alloc::{borrow::ToOwned, string::ToString, vec::Vec};

    use super::AddressSpace;
    use crate::address::{AddressError, LocalePrefixSet, SlugTable};
    use crate::i18n::Locale;

    fn tag(tag: &str) -> Locale {
        tag.parse().expect("a tag")
    }

    fn space() -> AddressSpace {
        let prefixes = LocalePrefixSet::new()
            .with("en", tag("en"))
            .expect("one prefix");
        let slugs = SlugTable::builder()
            .page("/", &[])
            .page("/about", &[])
            .build()
            .expect("a table");
        AddressSpace::new(prefixes, slugs)
            .with_unprefixed_locale(tag("nl"))
            .expect("Dutch has no prefix")
    }

    #[test]
    fn the_unprefixed_root_is_advertised_under_its_language() {
        assert_eq!(
            space().alternates("/"),
            [
                ("en".to_owned(), "/en".to_owned()),
                ("nl".to_owned(), "/".to_owned()),
                ("x-default".to_owned(), "/".to_owned()),
            ]
        );
    }

    #[test]
    fn a_page_rule_one_redirects_is_never_advertised_unprefixed() {
        let space = space();
        assert_eq!(space.resolve("/about", None).redirect_target(), Some("/"));
        assert!(
            space
                .alternates("/about")
                .iter()
                .all(|(hreflang, _)| hreflang != "nl"),
            "an address that redirects is not an alternate"
        );
    }

    #[test]
    fn the_unprefixed_root_is_served_in_its_language_whatever_the_reader_asked_for() {
        let resolved = space().resolve("/", Some(&tag("en")));
        assert_eq!(resolved.redirect_target(), None);
        assert_eq!(resolved.locale(), Some(&tag("nl")));
        assert!(
            resolved.locale_from_path(),
            "the address decided the language, so the response varies on nothing about the reader"
        );
    }

    #[test]
    fn without_a_declared_language_the_root_follows_the_reader_and_is_not_advertised() {
        let prefixes = LocalePrefixSet::new()
            .with("en", tag("en"))
            .expect("one prefix");
        let slugs = SlugTable::builder()
            .page("/", &[])
            .build()
            .expect("a table");
        let space = AddressSpace::new(prefixes, slugs);
        let negotiated = space.resolve("/", Some(&tag("de")));
        assert_eq!(negotiated.locale(), Some(&tag("de")));
        assert!(!negotiated.locale_from_path());
        assert_eq!(
            space.alternates("/"),
            [
                ("en".to_owned(), "/en".to_owned()),
                ("x-default".to_owned(), "/".to_owned()),
            ]
        );
    }

    #[test]
    fn a_language_served_under_a_prefix_cannot_also_be_unprefixed() {
        let prefixes = LocalePrefixSet::new()
            .with("nl", tag("nl"))
            .expect("one prefix");
        let refused = AddressSpace::new(prefixes, SlugTable::default())
            .with_unprefixed_locale(tag("nl"))
            .expect_err("one language, two addresses");
        assert_eq!(
            refused,
            AddressError::UnprefixedLocaleIsPrefixed {
                locale: "nl".to_string(),
                prefix: "nl".to_string(),
            }
        );
        assert_eq!(refused.code(), "ADDR_007");
    }
}

#[cfg(test)]
mod country_prefixes {
    //! Prefixes that are countries: one path vocabulary per country, read in two languages.

    use super::AddressSpace;
    use crate::address::{LocalePrefixSet, PageSlug, PrefixScope, SlugTable};
    #[cfg(not(feature = "std"))]
    use alloc::string::ToString;

    /// The Netherlands at `/nl` and `/nl-en`, both spelled in Dutch.
    fn netherlands() -> AddressSpace {
        let prefixes = LocalePrefixSet::new()
            .with_paths_in("nl", "nl-NL".parse().expect("tag"), PrefixScope::Full, "nl")
            .expect("nl")
            .with_paths_in(
                "nl-en",
                "en-NL".parse().expect("tag"),
                PrefixScope::Full,
                "nl",
            )
            .expect("nl-en");
        let slugs = SlugTable::builder()
            .page("/why", &[PageSlug::current("nl", "waarom")])
            .build()
            .expect("well formed");
        AddressSpace::new(prefixes, slugs)
    }

    #[test]
    fn one_country_spells_its_pages_one_way_in_every_language_it_serves() {
        let space = netherlands();
        assert_eq!(space.address("nl", "/why"), "/nl/waarom");
        assert_eq!(
            space.address("nl-en", "/why"),
            "/nl-en/waarom",
            "the English reading of a Dutch page is still that page, at that page's \
             address — a second spelling would make a link shared between two colleagues \
             who read different languages into two different URLs"
        );
    }

    #[test]
    fn the_english_stem_is_a_second_name_for_it_in_both() {
        let space = netherlands();
        assert_eq!(
            space.resolve("/nl-en/why", None).redirect_target(),
            Some("/nl-en/waarom")
        );
        assert_eq!(
            space.resolve("/nl/why", None).redirect_target(),
            Some("/nl/waarom")
        );
    }

    #[test]
    fn the_countrys_address_answers_in_the_readers_language() {
        let space = netherlands();
        let english = space.resolve("/nl-en/waarom", None);
        assert_eq!(english.canonical_path(), "/why");
        assert_eq!(english.redirect_target(), None);
        assert_eq!(
            english.locale().map(ToString::to_string),
            Some("en-NL".to_string()),
            "the *path* is the country's; the *locale* is still the reader's"
        );
    }

    /// The default is unchanged: a prefix that is a language spells its own paths.
    #[test]
    fn a_language_prefix_still_spells_its_own_paths() {
        let prefixes = LocalePrefixSet::new()
            .with("nl", "nl".parse().expect("tag"))
            .expect("nl")
            .with("en", "en".parse().expect("tag"))
            .expect("en");
        let slugs = SlugTable::builder()
            .page("/why", &[PageSlug::current("nl", "waarom")])
            .build()
            .expect("well formed");
        let space = AddressSpace::new(prefixes, slugs);
        assert_eq!(space.address("nl", "/why"), "/nl/waarom");
        assert_eq!(space.address("en", "/why"), "/en/why");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::PageSlug;

    fn locale(tag: &str) -> Locale {
        tag.parse().expect("a valid tag")
    }

    fn space() -> AddressSpace {
        let prefixes = LocalePrefixSet::new()
            .with("nl", locale("nl-NL"))
            .expect("nl")
            .with("nl-en", locale("en-NL"))
            .expect("nl-en")
            .with("be-fr", locale("fr-BE"))
            .expect("be-fr")
            .with_scope("pt", locale("en-PT"), PrefixScope::RootOnly)
            .expect("pt");
        let slugs = SlugTable::builder()
            .page(
                "/why",
                &[
                    PageSlug::current("nl", "waarom"),
                    PageSlug::current("fr", "pourquoi"),
                    PageSlug::retired("nl", "waarom-acme"),
                ],
            )
            .page("/trust", &[])
            .build()
            .expect("well formed");
        AddressSpace::new(prefixes, slugs)
    }

    #[test]
    fn the_canonical_address_serves_without_a_redirect() {
        let resolved = space().resolve("/nl/waarom", None);
        assert_eq!(resolved.canonical_path(), "/why");
        assert_eq!(resolved.prefix(), "/nl");
        assert_eq!(resolved.canonical_address(), "/nl/waarom");
        assert_eq!(resolved.redirect_target(), None);
        assert!(resolved.locale_from_path());
    }

    #[test]
    fn the_market_root_serves_the_index_page() {
        let resolved = space().resolve("/nl", None);
        assert_eq!(resolved.canonical_path(), "/");
        assert_eq!(resolved.canonical_address(), "/nl");
        assert_eq!(resolved.redirect_target(), None);
    }

    #[test]
    fn a_trailing_slash_is_not_a_second_address() {
        assert_eq!(space().resolve("/nl/", None).redirect_target(), None);
        assert_eq!(
            space().resolve("/nl/waarom/", None).redirect_target(),
            None,
            "normalised before comparison, so it is the same address"
        );
    }

    /// Rule 2: an untranslated stem under a translated locale is not the address.
    #[test]
    fn an_untranslated_stem_redirects_to_the_translated_one() {
        assert_eq!(
            space().resolve("/nl/why", None).redirect_target(),
            Some("/nl/waarom")
        );
    }

    #[test]
    fn a_retired_slug_redirects_to_the_current_one() {
        assert_eq!(
            space().resolve("/nl/waarom-acme", None).redirect_target(),
            Some("/nl/waarom")
        );
    }

    /// Rule 1: without a prefix the market is unknown, so the page is not guessed at.
    #[test]
    fn an_unprefixed_page_goes_home_rather_than_into_a_market() {
        let resolved = space().resolve("/why", None);
        assert_eq!(resolved.redirect_target(), Some("/"));
        assert!(!resolved.locale_from_path());
        assert_eq!(
            resolved.canonical_path(),
            "/why",
            "the path is left alone so a non-page route can still match it"
        );
    }

    #[test]
    fn the_home_address_is_served_rather_than_redirected() {
        assert_eq!(space().resolve("/", None).redirect_target(), None);
    }

    #[test]
    fn a_root_only_prefix_serves_one_address_and_folds_every_other_into_it() {
        let space = space();
        assert_eq!(space.resolve("/pt", None).redirect_target(), None);
        assert_eq!(space.resolve("/pt", None).canonical_path(), "/");
        for deep in ["/pt/waarom", "/pt/why", "/pt/a/b/c"] {
            assert_eq!(
                space.resolve(deep, None).redirect_target(),
                Some("/pt"),
                "{deep} must not become a second address"
            );
        }
    }

    #[test]
    fn a_language_without_a_prefix_falls_back_to_what_was_negotiated() {
        let fallback = locale("en");
        let resolved = space().resolve("/", Some(&fallback));
        assert_eq!(resolved.locale(), Some(&fallback));
        assert!(
            !resolved.locale_from_path(),
            "a negotiated locale still varies per reader"
        );
    }

    #[test]
    fn a_prefix_locale_beats_the_negotiated_one() {
        let fallback = locale("de");
        let resolved = space().resolve("/nl/waarom", Some(&fallback));
        assert_eq!(
            resolved.locale().map(Locale::to_string),
            Some("nl-NL".into())
        );
        assert!(resolved.locale_from_path());
    }

    /// A link must agree with what `resolve` calls canonical.
    #[test]
    fn address_agrees_with_resolve_about_the_canonical_address() {
        let space = space();
        for (prefix, canonical) in [("nl", "/why"), ("nl-en", "/why"), ("be-fr", "/why")] {
            let address = space.address(prefix, canonical);
            let resolved = space.resolve(&address, None);
            assert_eq!(
                resolved.redirect_target(),
                None,
                "{address} is not its own canonical address"
            );
            assert_eq!(resolved.canonical_path(), canonical);
        }
    }

    #[test]
    fn every_language_gets_its_own_spelling_of_one_page() {
        let space = space();
        assert_eq!(space.address("nl", "/why"), "/nl/waarom");
        assert_eq!(space.address("be-fr", "/why"), "/be-fr/pourquoi");
        assert_eq!(
            space.address("nl-en", "/why"),
            "/nl-en/why",
            "English declared no slug, so it keeps the stem"
        );
    }

    #[test]
    fn alternates_name_every_open_market_and_a_default() {
        let alternates = space().alternates("/why");
        // Prefix order (`be-fr` < `nl` < `nl-en`), with `x-default` last.
        assert_eq!(
            alternates,
            [
                ("fr-BE".to_string(), "/be-fr/pourquoi".to_string()),
                ("nl-NL".to_string(), "/nl/waarom".to_string()),
                ("en-NL".to_string(), "/nl-en/why".to_string()),
                ("x-default".to_string(), "/".to_string()),
            ]
        );
    }

    /// A prefix that serves one page is `noindex`, so it is no alternate.
    #[test]
    fn a_root_only_prefix_is_never_advertised_as_an_alternate() {
        assert!(
            !space()
                .alternates("/why")
                .iter()
                .any(|(tag, _)| tag == "en-PT"),
            "a closed market must not appear in hreflang"
        );
    }

    #[test]
    fn an_empty_space_leaves_every_path_alone() {
        let space = AddressSpace::default().with_unaddressed(Vec::<String>::new());
        let resolved = space.resolve("/why", None);
        assert_eq!(resolved.canonical_path(), "/why");
        assert_eq!(
            resolved.redirect_target(),
            Some("/"),
            "with no prefixes and no unaddressed paths, everything is off-address"
        );
    }

    #[test]
    fn a_surface_may_declare_more_than_one_unaddressed_path() {
        let space = space().with_unaddressed(["/", "/health"]);
        assert_eq!(space.resolve("/health", None).redirect_target(), None);
        assert_eq!(space.resolve("/why", None).redirect_target(), Some("/"));
    }
}
