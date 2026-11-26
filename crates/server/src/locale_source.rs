/// Where an HTTP boundary reads the caller's language preference from.
///
/// ```
/// # extern crate fundamental_server;
/// use fundamental_server::LocaleSource;
///
/// // Honour a switcher's cookie, falling back to what the browser asked for.
/// let switchable = LocaleSource::cookie_then_accept_language("locale");
/// assert!(switchable.reads_cookie() && switchable.reads_accept_language());
/// // The default reads the header only.
/// assert_eq!(LocaleSource::default(), LocaleSource::AcceptLanguage);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LocaleSource {
    /// The `Accept-Language` header only. The default.
    #[default]
    AcceptLanguage,
    /// A named cookie only, for a surface where the language is always an explicit choice.
    Cookie {
        /// The cookie name, matched case-sensitively.
        name: String,
    },
    /// The named cookie first, then the `Accept-Language` header.
    CookieThenAcceptLanguage {
        /// The cookie name, matched case-sensitively.
        cookie: String,
    },

    /// A leading path segment, resolved by the router's [`AddressSpace`](fundamental_core::address::AddressSpace).
    ///
    /// With no `fallback`, [`LocalePolicy::vary`](crate::LocalePolicy::vary) declares nothing
    /// for language: the URL is the whole cache key.
    Path {
        /// What decides the locale for a path with no prefix; `None` means the policy's default.
        fallback: Option<Box<LocaleSource>>,
    },
}

impl LocaleSource {
    /// Reads the preference from the cookie named `name`, and nowhere else.
    #[must_use]
    pub fn cookie(name: impl Into<String>) -> Self {
        Self::Cookie { name: name.into() }
    }

    /// Reads the cookie named `name` first, then the `Accept-Language` header.
    #[must_use]
    pub fn cookie_then_accept_language(name: impl Into<String>) -> Self {
        Self::CookieThenAcceptLanguage {
            cookie: name.into(),
        }
    }

    /// Reads the locale from the address, and from nowhere else.
    #[must_use]
    pub fn path() -> Self {
        Self::Path { fallback: None }
    }

    /// Reads the locale from the address, falling back to `source` where a path carries no prefix.
    #[must_use]
    pub fn path_then(source: LocaleSource) -> Self {
        Self::Path {
            fallback: Some(Box::new(source)),
        }
    }

    /// The configured cookie name, if this source reads one.
    #[must_use]
    pub fn cookie_name(&self) -> Option<&str> {
        match self {
            Self::AcceptLanguage => None,
            Self::Cookie { name } | Self::CookieThenAcceptLanguage { cookie: name } => Some(name),
            Self::Path { fallback } => fallback.as_ref().and_then(|inner| inner.cookie_name()),
        }
    }

    /// Whether the address decides the locale.
    #[must_use]
    pub fn reads_path(&self) -> bool {
        matches!(self, Self::Path { .. })
    }

    /// Whether this source reads a cookie, which a response must then vary on.
    #[must_use]
    pub fn reads_cookie(&self) -> bool {
        self.cookie_name().is_some()
    }

    /// Whether this source reads the `Accept-Language` header.
    #[must_use]
    pub fn reads_accept_language(&self) -> bool {
        match self {
            Self::AcceptLanguage | Self::CookieThenAcceptLanguage { .. } => true,
            Self::Cookie { .. } => false,
            Self::Path { fallback } => fallback
                .as_ref()
                .is_some_and(|inner| inner.reads_accept_language()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_reads_the_header_and_no_cookie() {
        let source = LocaleSource::default();
        assert_eq!(source, LocaleSource::AcceptLanguage);
        assert!(source.reads_accept_language());
        assert!(!source.reads_cookie());
        assert_eq!(source.cookie_name(), None);
    }

    #[test]
    fn a_cookie_source_reads_only_the_cookie() {
        let source = LocaleSource::cookie("locale");
        assert_eq!(source.cookie_name(), Some("locale"));
        assert!(source.reads_cookie());
        assert!(
            !source.reads_accept_language(),
            "a surface that wants only an explicit choice must not fall back to the browser's"
        );
    }

    #[test]
    fn a_cookie_then_header_source_reads_both() {
        let source = LocaleSource::cookie_then_accept_language("locale");
        assert_eq!(source.cookie_name(), Some("locale"));
        assert!(source.reads_cookie());
        assert!(source.reads_accept_language());
    }

    #[test]
    fn a_path_source_reads_neither_a_header_nor_a_cookie() {
        let source = LocaleSource::path();
        assert!(source.reads_path());
        assert!(!source.reads_cookie());
        assert!(!source.reads_accept_language());
        assert_eq!(source.cookie_name(), None);
    }

    #[test]
    fn a_path_source_with_a_fallback_reads_what_the_fallback_reads() {
        let source = LocaleSource::path_then(LocaleSource::cookie_then_accept_language("locale"));
        assert!(source.reads_path());
        assert!(source.reads_cookie());
        assert!(source.reads_accept_language());
        assert_eq!(source.cookie_name(), Some("locale"));
    }

    #[test]
    fn only_a_path_source_reads_the_path() {
        for source in [
            LocaleSource::AcceptLanguage,
            LocaleSource::cookie("locale"),
            LocaleSource::cookie_then_accept_language("locale"),
        ] {
            assert!(!source.reads_path(), "{source:?}");
        }
    }
}
