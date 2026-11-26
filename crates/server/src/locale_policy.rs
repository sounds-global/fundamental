use fundamental_core::i18n::Locale;
use tracing::debug;

use crate::accept_language::{LanguagePreference, preferences};
use crate::cookie_lookup::CookieLookup;
use crate::locale_source::LocaleSource;

/// The languages a surface offers, and how a request's preference selects one.
///
/// Negotiation cannot fail: anything not on offer gets the default.
///
/// ```
/// # extern crate fundamental_server;
/// # extern crate fundamental_core;
/// use fundamental_core::i18n::Locale;
/// use fundamental_server::LocalePolicy;
///
/// let dutch: Locale = "nl".parse().expect("a language tag");
/// let english: Locale = "en".parse().expect("a language tag");
/// let policy = LocalePolicy::new(english.clone()).with_supported(vec![dutch.clone()]);
///
/// // A Flemish visitor is served the Dutch catalog, not the English default.
/// assert_eq!(policy.negotiate(Some("nl-BE,nl;q=0.9"), [].into_iter()), dutch);
/// // A language nothing is offered in falls back, rather than 406-ing a reader.
/// assert_eq!(policy.negotiate(Some("ja"), [].into_iter()), english);
/// ```
#[derive(Debug, Clone)]
pub struct LocalePolicy {
    source: LocaleSource,
    supported: Vec<Locale>,
    default: Locale,
    regions: Vec<Locale>,
}

impl LocalePolicy {
    /// A policy offering only `default`, reading the `Accept-Language` header.
    #[must_use]
    pub fn new(default: Locale) -> Self {
        Self {
            source: LocaleSource::default(),
            supported: vec![default.clone()],
            default,
            regions: Vec::new(),
        }
    }

    /// Says which region `written`'s language is written for (`en-GB`); negotiation and addresses are unchanged.
    #[must_use]
    pub fn with_region(mut self, written: Locale) -> Self {
        self.regions
            .retain(|known| known.language() != written.language());
        self.regions.push(written);
        self
    }

    /// `locale`'s language with the region it is written for, or `locale` as it is.
    #[must_use]
    pub fn regional(&self, locale: &Locale) -> Locale {
        self.regions
            .iter()
            .find(|known| known.language() == locale.language())
            .unwrap_or(locale)
            .clone()
    }

    /// Sets the languages this surface can answer in; the default is re-added if left out.
    #[must_use]
    pub fn with_supported(mut self, supported: Vec<Locale>) -> Self {
        self.supported = supported;
        if !self.supported.contains(&self.default) {
            self.supported.push(self.default.clone());
        }
        self
    }

    /// Sets where the caller's preference is read from.
    #[must_use]
    pub fn with_source(mut self, source: LocaleSource) -> Self {
        self.source = source;
        self
    }

    /// The language served when a request expresses no usable preference.
    #[must_use]
    pub fn default_locale(&self) -> &Locale {
        &self.default
    }

    /// The languages this surface can answer in. Always contains the default.
    #[must_use]
    pub fn supported(&self) -> &[Locale] {
        &self.supported
    }

    /// Where the caller's preference is read from.
    #[must_use]
    pub fn source(&self) -> &LocaleSource {
        &self.source
    }

    /// The request headers a response produced under this policy varies on; empty for a path source.
    #[must_use]
    pub fn vary(&self) -> &'static [&'static str] {
        // Exhaustive: a catch-all would over-declare for a path source and cost its cache efficiency.
        match (
            self.source.reads_cookie(),
            self.source.reads_accept_language(),
        ) {
            (true, true) => &["Accept-Language", "Cookie"],
            (true, false) => &["Cookie"],
            (false, true) => &["Accept-Language"],
            (false, false) => &[],
        }
    }

    /// Selects the language to answer this request in, from every `Cookie` field and `Accept-Language`.
    ///
    /// A cookie naming an unoffered language falls through to the header.
    pub fn negotiate<'a>(
        &self,
        accept_language: Option<&str>,
        cookie_fields: impl Iterator<Item = &'a str>,
    ) -> Locale {
        if let Some(name) = self.source.cookie_name()
            && let CookieLookup::Found(value) = CookieLookup::find(cookie_fields, name)
        {
            match value
                .parse::<Locale>()
                .ok()
                .and_then(|wanted| self.select(&[LanguagePreference::Tag(wanted)]))
            {
                Some(chosen) => return chosen,
                None => debug!(
                    cookie = name,
                    "the locale cookie named a language this surface does not serve"
                ),
            }
        }

        if self.source.reads_accept_language()
            && let Some(header) = accept_language
            && let Some(chosen) = self.select(&preferences(header))
        {
            return chosen;
        }

        self.default.clone()
    }

    /// The first offered locale any preference matches, each preference widened before the next.
    fn select(&self, preferences: &[LanguagePreference]) -> Option<Locale> {
        for preference in preferences {
            let wanted = match preference {
                LanguagePreference::Any => return Some(self.default.clone()),
                LanguagePreference::Tag(wanted) => wanted,
            };

            let language_only = wanted.language_fallback();
            let matched = self
                .supported
                .iter()
                .find(|offered| *offered == wanted)
                .or_else(|| {
                    self.supported
                        .iter()
                        .find(|offered| **offered == language_only)
                })
                .or_else(|| {
                    self.supported
                        .iter()
                        .find(|offered| offered.language() == wanted.language())
                });

            if let Some(matched) = matched {
                return Some(matched.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locale(tag: &str) -> Locale {
        tag.parse().expect("a language tag")
    }

    fn policy() -> LocalePolicy {
        LocalePolicy::new(locale("en")).with_supported(vec![locale("nl"), locale("en")])
    }

    #[test]
    fn a_language_is_written_for_the_region_declared_for_it() {
        let policy = policy()
            .with_region(locale("en-US"))
            .with_region(locale("en-GB"))
            .with_region(locale("nl-NL"));
        assert_eq!(policy.regional(&locale("en")), locale("en-GB"));
        assert_eq!(policy.regional(&locale("nl")), locale("nl-NL"));
        assert_eq!(policy.regional(&locale("de")), locale("de"));
        assert_eq!(
            policy.negotiate(Some("nl"), [].into_iter()),
            locale("nl"),
            "a region moves no page"
        );
    }

    #[test]
    fn the_default_is_always_on_offer_even_if_a_caller_omits_it() {
        let policy = LocalePolicy::new(locale("en")).with_supported(vec![locale("nl")]);
        assert!(
            policy.supported().contains(&locale("en")),
            "a policy that could select nothing would answer in a language it cannot render"
        );
    }

    #[test]
    fn an_exact_tag_is_selected() {
        assert_eq!(policy().negotiate(Some("nl"), [].into_iter()), locale("nl"));
    }

    #[test]
    fn a_regional_tag_falls_back_to_its_language() {
        assert_eq!(
            policy().negotiate(Some("nl-BE"), [].into_iter()),
            locale("nl")
        );
    }

    #[test]
    fn a_language_is_answered_by_an_offered_regional_variant() {
        let policy = LocalePolicy::new(locale("en")).with_supported(vec![locale("nl-BE")]);
        assert_eq!(
            policy.negotiate(Some("nl"), [].into_iter()),
            locale("nl-BE")
        );
    }

    #[test]
    fn the_caller_ranking_decides_between_two_offered_languages() {
        assert_eq!(
            policy().negotiate(Some("en;q=0.5, nl;q=0.9"), [].into_iter()),
            locale("nl")
        );
        assert_eq!(
            policy().negotiate(Some("en;q=0.9, nl;q=0.5"), [].into_iter()),
            locale("en")
        );
    }

    #[test]
    fn a_language_nothing_is_offered_in_falls_back_to_the_default() {
        assert_eq!(policy().negotiate(Some("ja"), [].into_iter()), locale("en"));
    }

    #[test]
    fn an_unoffered_first_choice_yields_to_an_offered_second() {
        assert_eq!(
            policy().negotiate(Some("ja, nl;q=0.5"), [].into_iter()),
            locale("nl")
        );
    }

    #[test]
    fn the_wildcard_selects_the_default() {
        assert_eq!(policy().negotiate(Some("*"), [].into_iter()), locale("en"));
    }

    #[test]
    fn no_header_at_all_is_the_default() {
        assert_eq!(policy().negotiate(None, [].into_iter()), locale("en"));
        assert_eq!(policy().negotiate(Some(""), [].into_iter()), locale("en"));
    }

    #[test]
    fn a_cookie_beats_the_header_when_the_source_reads_one() {
        let policy = policy().with_source(LocaleSource::cookie_then_accept_language("locale"));
        assert_eq!(
            policy.negotiate(Some("en"), ["locale=nl"].into_iter()),
            locale("nl"),
            "an explicit choice must beat the browser's configuration"
        );
    }

    #[test]
    fn a_cookie_naming_an_unoffered_language_falls_through_to_the_header() {
        let policy = policy().with_source(LocaleSource::cookie_then_accept_language("locale"));
        assert_eq!(
            policy.negotiate(Some("nl"), ["locale=ja"].into_iter()),
            locale("nl")
        );
        assert_eq!(
            policy.negotiate(Some("nl"), ["locale=not-a-tag"].into_iter()),
            locale("nl")
        );
    }

    // Two languages is a muddle, not an attack: unlike two sessions, it falls through.
    #[test]
    fn an_ambiguous_locale_cookie_falls_through_rather_than_failing() {
        let policy = policy().with_source(LocaleSource::cookie_then_accept_language("locale"));
        assert_eq!(
            policy.negotiate(Some("nl"), ["locale=nl; locale=en"].into_iter()),
            locale("nl")
        );
    }

    #[test]
    fn a_cookie_only_source_ignores_the_header() {
        let policy = policy().with_source(LocaleSource::cookie("locale"));
        assert_eq!(policy.negotiate(Some("nl"), [].into_iter()), locale("en"));
        assert_eq!(
            policy.negotiate(Some("nl"), ["locale=nl"].into_iter()),
            locale("nl")
        );
    }

    #[test]
    fn vary_names_every_header_the_selection_read() {
        assert_eq!(policy().vary(), ["Accept-Language"]);
        assert_eq!(
            policy().with_source(LocaleSource::cookie("locale")).vary(),
            ["Cookie"]
        );
        assert_eq!(
            policy()
                .with_source(LocaleSource::cookie_then_accept_language("locale"))
                .vary(),
            ["Accept-Language", "Cookie"]
        );
    }

    #[test]
    fn a_path_source_declares_no_language_vary_at_all() {
        let policy = policy().with_source(LocaleSource::path());
        assert!(
            policy.vary().is_empty(),
            "declaring one would split the cache once per Accept-Language value — at \
             scale, once per reader — across byte-identical responses, got {:?}",
            policy.vary()
        );
    }

    // Prefixed responses opt out per response, which is the router's job.
    #[test]
    fn a_path_source_with_a_fallback_declares_the_fallbacks_headers() {
        assert_eq!(
            policy()
                .with_source(LocaleSource::path_then(
                    LocaleSource::cookie_then_accept_language("locale")
                ))
                .vary(),
            ["Accept-Language", "Cookie"]
        );
    }
}
