//! [`PageSlug`] — one page's path segment in one language.

use crate::address::SlugStatus;

/// A page's path segment in one language.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::PageSlug;
///
/// // pages/why.rs declares its Dutch and Swedish addresses.
/// const SLUGS: &[PageSlug] = &[
///     PageSlug::current("nl", "waarom"),
///     PageSlug::current("sv", "varfor"),
///     PageSlug::retired("nl", "waarom-acme"),
/// ];
/// assert_eq!(SLUGS[0].slug(), "waarom");
/// assert!(!SLUGS[2].is_current());
/// ```
///
/// The language is the primary subtag only: `nl`, not `nl-BE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSlug {
    language: &'static str,
    slug: &'static str,
    status: SlugStatus,
}

impl PageSlug {
    /// The page's address in `language`.
    #[must_use]
    pub const fn current(language: &'static str, slug: &'static str) -> Self {
        Self {
            language,
            slug,
            status: SlugStatus::Current,
        }
    }

    /// A spelling this page used to answer at in `language`.
    ///
    /// Keep these forever: deleting one turns a permanent redirect into a `404`.
    #[must_use]
    pub const fn retired(language: &'static str, slug: &'static str) -> Self {
        Self {
            language,
            slug,
            status: SlugStatus::Retired,
        }
    }

    /// The primary language subtag this slug is written in.
    #[must_use]
    pub const fn language(&self) -> &'static str {
        self.language
    }

    /// The path segment.
    #[must_use]
    pub const fn slug(&self) -> &'static str {
        self.slug
    }

    /// Whether this spelling is the current address rather than a retired one.
    #[must_use]
    pub const fn is_current(&self) -> bool {
        self.status.is_current()
    }

    /// Whether this is a well-formed single path segment.
    ///
    /// Refuses `/`, `?`, `#`, `%` (a slug is declared decoded) and invisible characters.
    #[must_use]
    pub fn is_one_segment(&self) -> bool {
        !self.slug.is_empty()
            && !self.language.is_empty()
            && !self
                .slug
                .chars()
                .any(|c| matches!(c, '/' | '?' | '#' | '%') || is_invisible(c))
    }
}

/// Whether `c` would not be seen in an address: whitespace, control and format characters.
fn is_invisible(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(c,
            '\u{00ad}'                  // soft hyphen
            | '\u{061c}'                // Arabic letter mark
            | '\u{180e}'                // Mongolian vowel separator
            | '\u{200b}'..='\u{200f}'   // zero-width space … right-to-left mark
            | '\u{202a}'..='\u{202e}'   // bidi embeddings and overrides
            | '\u{2060}'..='\u{2064}'   // word joiner … invisible plus
            | '\u{2066}'..='\u{2069}'   // bidi isolates
            | '\u{feff}'                // byte-order mark
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slug_is_const_constructible_so_a_page_can_declare_it() {
        const SLUG: PageSlug = PageSlug::current("nl", "waarom");
        assert_eq!(SLUG.language(), "nl");
        assert_eq!(SLUG.slug(), "waarom");
        assert!(SLUG.is_current());
    }

    #[test]
    fn a_retired_slug_reports_itself_as_such() {
        assert!(!PageSlug::retired("nl", "oud").is_current());
    }

    #[test]
    fn a_slug_that_is_not_one_segment_is_recognisable() {
        assert!(PageSlug::current("nl", "waarom").is_one_segment());
        assert!(!PageSlug::current("nl", "").is_one_segment());
        assert!(!PageSlug::current("", "waarom").is_one_segment());
        for bad in ["a/b", "a?b", "a#b", "a%2Fb", "100%"] {
            assert!(
                !PageSlug::current("nl", bad).is_one_segment(),
                "'{bad}' escapes its segment"
            );
        }
    }

    /// Two addresses that look identical are one address in practice.
    #[test]
    fn a_slug_in_another_script_is_one_segment_and_an_invisible_one_is_not() {
        for word in ["ログイン", "вход", "دخول", "登录", "प्रवेश"] {
            assert!(PageSlug::current("ja", word).is_one_segment(), "{word}");
        }
        for bad in ["log in", "log\tin", "log\u{200b}in", "\u{202e}nigol"] {
            assert!(
                !PageSlug::current("nl", bad).is_one_segment(),
                "{bad:?} is not visibly one word"
            );
        }
    }
}
