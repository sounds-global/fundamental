//! [`LocalePrefixSet`] — the leading path segments that select a locale.

#[cfg(not(feature = "std"))]
use alloc::{borrow::ToOwned, string::String, vec::Vec};

use crate::address::{AddressError, PrefixMatch, PrefixScope};
use crate::i18n::Locale;

/// One prefix's entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PrefixEntry {
    prefix: String,
    locale: Locale,
    /// What this prefix's addresses are spelled in; see [`PrefixMatch::path_language`].
    path_language: String,
    scope: PrefixScope,
}

/// The leading path segments a surface answers, each naming a [`Locale`].
///
/// What the segments mean (a country, a brand, a tenant) is the application's business;
/// because the prefix decides the locale, the URL is the whole cache key.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::{LocalePrefixSet, PrefixScope};
/// use fundamental_core::i18n::Locale;
///
/// let set = LocalePrefixSet::new()
///     .with("nl", "nl-NL".parse::<Locale>().expect("tag"))?
///     .with("nl-en", "en-NL".parse::<Locale>().expect("tag"))?
///     .with_scope("pt", "en-PT".parse::<Locale>().expect("tag"), PrefixScope::RootOnly)?;
///
/// let matched = set.resolve("/nl/waarom").expect("an open market");
/// assert_eq!(matched.prefix(), "nl");
/// assert_eq!(matched.locale().to_string(), "nl-NL");
/// assert_eq!(matched.rest(), "/waarom");
///
/// // A segment that names no prefix resolves to nothing — it is not a market.
/// assert!(set.resolve("/why").is_none());
/// # Ok::<(), fundamental_core::address::AddressError>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalePrefixSet {
    /// Sorted by `prefix`, so `binary_search_by` needs no allocation.
    entries: Vec<PrefixEntry>,
}

impl LocalePrefixSet {
    /// An empty set: no path segment selects a locale.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `prefix`, selecting `locale`, serving the whole route tree.
    ///
    /// # Errors
    /// [`AddressError::PrefixNotOneSegment`] if `prefix` is empty or contains `/`;
    /// [`AddressError::DuplicatePrefix`] if it is already registered.
    pub fn with(self, prefix: &str, locale: Locale) -> Result<Self, AddressError> {
        self.with_scope(prefix, locale, PrefixScope::Full)
    }

    /// Adds `prefix`, selecting `locale`, serving `scope`.
    ///
    /// # Errors
    /// [`AddressError::PrefixNotOneSegment`] if `prefix` is empty or contains `/`;
    /// [`AddressError::DuplicatePrefix`] if it is already registered.
    pub fn with_scope(
        self,
        prefix: &str,
        locale: Locale,
        scope: PrefixScope,
    ) -> Result<Self, AddressError> {
        let spelled_in = locale.language().to_owned();
        self.with_paths_in(prefix, locale, scope, &spelled_in)
    }

    /// Adds `prefix`, selecting `locale`, serving `scope`, with its addresses spelled
    /// in `path_language`.
    ///
    /// # Errors
    /// [`AddressError::PrefixNotOneSegment`] if `prefix` is empty or contains `/`;
    /// [`AddressError::DuplicatePrefix`] if it is already registered.
    pub fn with_paths_in(
        mut self,
        prefix: &str,
        locale: Locale,
        scope: PrefixScope,
        path_language: &str,
    ) -> Result<Self, AddressError> {
        if prefix.is_empty() || prefix.contains('/') {
            return Err(AddressError::PrefixNotOneSegment {
                prefix: prefix.to_owned(),
            });
        }
        match self
            .entries
            .binary_search_by(|entry| entry.prefix.as_str().cmp(prefix))
        {
            Ok(_) => Err(AddressError::DuplicatePrefix {
                prefix: prefix.to_owned(),
            }),
            Err(at) => {
                self.entries.insert(
                    at,
                    PrefixEntry {
                        prefix: prefix.to_owned(),
                        locale,
                        path_language: path_language.to_owned(),
                        scope,
                    },
                );
                Ok(self)
            }
        }
    }

    /// Splits `path` into `(prefix, locale, remainder)` when its first segment is
    /// a registered prefix.
    ///
    /// The remainder always begins with `/` and is `"/"` for the prefix's own root.
    #[must_use]
    pub fn resolve<'a, 'p>(&'p self, path: &'a str) -> Option<PrefixMatch<'p, 'a>> {
        let trimmed = path.strip_prefix('/').unwrap_or(path);
        let (head, rest) = match trimmed.find('/') {
            Some(at) => (&trimmed[..at], &trimmed[at..]),
            None => (trimmed, "/"),
        };
        let entry = self.entry(head)?;
        let rest = if rest.is_empty() { "/" } else { rest };
        Some(PrefixMatch::new(
            entry.prefix.as_str(),
            &entry.locale,
            entry.path_language.as_str(),
            entry.scope,
            rest,
        ))
    }

    /// The language `prefix`'s addresses are spelled in, if it is registered.
    #[must_use]
    pub fn path_language_of(&self, prefix: &str) -> Option<&str> {
        self.entry(prefix).map(|entry| entry.path_language.as_str())
    }

    /// The scope registered for `prefix`, if any.
    #[must_use]
    pub fn scope_of(&self, prefix: &str) -> Option<PrefixScope> {
        self.entry(prefix).map(|entry| entry.scope)
    }

    /// The locale registered for `prefix`, if any.
    #[must_use]
    pub fn locale_of(&self, prefix: &str) -> Option<&Locale> {
        self.entry(prefix).map(|entry| &entry.locale)
    }

    /// Every registered `(prefix, locale, scope)`, in prefix order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Locale, PrefixScope)> {
        self.entries
            .iter()
            .map(|entry| (entry.prefix.as_str(), &entry.locale, entry.scope))
    }

    /// Every locale this set can answer in, in prefix order.
    #[must_use]
    pub fn locales(&self) -> Vec<Locale> {
        self.entries
            .iter()
            .map(|entry| entry.locale.clone())
            .collect()
    }

    /// Whether nothing is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many prefixes are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    fn entry(&self, prefix: &str) -> Option<&PrefixEntry> {
        self.entries
            .binary_search_by(|entry| entry.prefix.as_str().cmp(prefix))
            .ok()
            .and_then(|at| self.entries.get(at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(feature = "std"))]
    use alloc::string::ToString;

    fn locale(tag: &str) -> Locale {
        tag.parse().expect("a valid tag")
    }

    fn set() -> LocalePrefixSet {
        LocalePrefixSet::new()
            .with("nl", locale("nl-NL"))
            .expect("nl")
            .with("nl-en", locale("en-NL"))
            .expect("nl-en")
            .with("be-nl", locale("nl-BE"))
            .expect("be-nl")
            .with("be-fr", locale("fr-BE"))
            .expect("be-fr")
            .with_scope("pt", locale("en-PT"), PrefixScope::RootOnly)
            .expect("pt")
    }

    #[test]
    fn a_market_collapses_to_its_primary_language() {
        let set = set();
        let matched = set.resolve("/nl/waarom").expect("resolves");
        let (prefix, locale, rest) = (matched.prefix(), matched.locale(), matched.rest());
        assert_eq!(prefix, "nl");
        assert_eq!(locale.to_string(), "nl-NL");
        assert_eq!(rest, "/waarom");
    }

    #[test]
    fn a_second_language_expands_into_its_own_prefix() {
        let set = set();
        let matched = set.resolve("/nl-en/why").expect("resolves");
        let (prefix, locale, rest) = (matched.prefix(), matched.locale(), matched.rest());
        assert_eq!(prefix, "nl-en");
        assert_eq!(locale.to_string(), "en-NL");
        assert_eq!(rest, "/why");
    }

    /// Belgium has no primary language, so there is no bare `/be`.
    #[test]
    fn a_market_with_no_primary_has_only_expanded_prefixes() {
        let set = set();
        assert!(set.resolve("/be-nl/waarom").is_some());
        assert!(set.resolve("/be-fr/pourquoi").is_some());
        assert!(
            set.resolve("/be/waarom").is_none(),
            "a bare /be would have to pick one of Belgium's languages for the reader"
        );
    }

    #[test]
    fn the_prefix_root_yields_a_remainder_of_slash() {
        for path in ["/nl", "/nl/"] {
            let rest = set().resolve(path).expect("resolves").rest();
            assert_eq!(rest, "/", "{path} should read as the market root");
        }
    }

    #[test]
    fn an_unregistered_segment_is_not_a_prefix() {
        assert!(set().resolve("/why").is_none());
        assert!(set().resolve("/").is_none());
        assert!(set().resolve("/legal/privacy").is_none());
    }

    #[test]
    fn a_root_only_prefix_reports_its_scope() {
        assert_eq!(set().scope_of("pt"), Some(PrefixScope::RootOnly));
        assert_eq!(set().scope_of("nl"), Some(PrefixScope::Full));
        assert_eq!(set().scope_of("zz"), None);
    }

    #[test]
    fn a_duplicate_prefix_is_refused_rather_than_overwritten() {
        let error = set()
            .with("nl", locale("de-DE"))
            .expect_err("the loser would boot fine and be unreachable");
        assert!(matches!(error, AddressError::DuplicatePrefix { .. }));
    }

    #[test]
    fn a_prefix_must_be_exactly_one_segment() {
        for bad in ["", "nl/be", "/nl"] {
            let error = LocalePrefixSet::new()
                .with(bad, locale("nl"))
                .expect_err("'{bad}' is not one segment");
            assert!(matches!(error, AddressError::PrefixNotOneSegment { .. }));
        }
    }

    #[test]
    fn insertion_order_does_not_change_what_resolves() {
        let forwards = LocalePrefixSet::new()
            .with("a", locale("en"))
            .expect("a")
            .with("m", locale("nl"))
            .expect("m")
            .with("z", locale("de"))
            .expect("z");
        let backwards = LocalePrefixSet::new()
            .with("z", locale("de"))
            .expect("z")
            .with("m", locale("nl"))
            .expect("m")
            .with("a", locale("en"))
            .expect("a");
        assert_eq!(forwards, backwards);
        for prefix in ["a", "m", "z"] {
            assert!(backwards.locale_of(prefix).is_some(), "{prefix} is missing");
        }
    }

    #[test]
    fn locales_are_the_one_list_a_policy_is_built_from() {
        let tags: Vec<String> = set().locales().iter().map(Locale::to_string).collect();
        assert_eq!(tags, ["fr-BE", "nl-BE", "nl-NL", "en-NL", "en-PT"]);
    }
}
