//! [`ResolvedAddress`] — what an address space made of one requested path.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use crate::i18n::Locale;

/// The answer to "what does this request address, and is it the right address?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAddress {
    prefix: String,
    locale: Option<Locale>,
    locale_from_path: bool,
    canonical_path: String,
    canonical_address: String,
    redirect_target: Option<String>,
}

impl ResolvedAddress {
    /// A path whose first segment named no prefix.
    pub(super) fn unprefixed(
        canonical_path: String,
        locale: Option<Locale>,
        redirect_target: Option<String>,
    ) -> Self {
        Self {
            prefix: String::new(),
            locale,
            locale_from_path: false,
            canonical_address: canonical_path.clone(),
            canonical_path,
            redirect_target,
        }
    }

    /// An unaddressed path on a surface that declared the language unprefixed pages are served in.
    pub(super) fn unprefixed_in(canonical_path: String, locale: Locale) -> Self {
        Self {
            prefix: String::new(),
            locale: Some(locale),
            locale_from_path: true,
            canonical_address: canonical_path.clone(),
            canonical_path,
            redirect_target: None,
        }
    }

    /// A path whose first segment selected a locale.
    pub(super) fn prefixed(
        prefix: String,
        locale: Locale,
        canonical_path: String,
        canonical_address: String,
        redirect_target: Option<String>,
    ) -> Self {
        Self {
            prefix,
            locale: Some(locale),
            locale_from_path: true,
            canonical_path,
            canonical_address,
            redirect_target,
        }
    }

    /// The locale this request is answered in, if one was decided at all.
    #[must_use]
    pub fn locale(&self) -> Option<&Locale> {
        self.locale.as_ref()
    }

    /// Whether the **path** decided the locale rather than a header or a cookie.
    ///
    /// When true the URL is the whole cache key and a response needs no `Vary`.
    #[must_use]
    pub fn locale_from_path(&self) -> bool {
        self.locale_from_path
    }

    /// The leading segment, with its slash (`"/nl"`), or empty when there was none.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The path to match against the route table.
    #[must_use]
    pub fn canonical_path(&self) -> &str {
        &self.canonical_path
    }

    /// The one address that serves this page in this locale.
    #[must_use]
    pub fn canonical_address(&self) -> &str {
        &self.canonical_address
    }

    /// Where to send the reader, when what they asked for was not the one address.
    ///
    /// Advisory: the router redirects only once it knows the path matched a page.
    #[must_use]
    pub fn redirect_target(&self) -> Option<&str> {
        self.redirect_target.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(feature = "std"))]
    use alloc::string::ToString;

    #[test]
    fn an_unprefixed_address_carries_no_prefix_and_no_path_locale() {
        let resolved = ResolvedAddress::unprefixed(
            "/why".to_string(),
            Some("en".parse().expect("tag")),
            Some("/".to_string()),
        );
        assert_eq!(resolved.prefix(), "");
        assert!(!resolved.locale_from_path());
        assert_eq!(resolved.canonical_path(), "/why");
        assert_eq!(resolved.redirect_target(), Some("/"));
    }

    #[test]
    fn a_prefixed_address_reports_that_the_path_decided_the_locale() {
        let resolved = ResolvedAddress::prefixed(
            "/nl".to_string(),
            "nl-NL".parse().expect("tag"),
            "/why".to_string(),
            "/nl/waarom".to_string(),
            None,
        );
        assert!(resolved.locale_from_path());
        assert_eq!(resolved.prefix(), "/nl");
        assert_eq!(resolved.canonical_address(), "/nl/waarom");
        assert_eq!(resolved.redirect_target(), None);
    }
}
