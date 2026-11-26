//! [`PrefixMatch`] — what a leading path segment matched.

use crate::address::PrefixScope;
use crate::i18n::Locale;

/// The entry a leading path segment selected, and the path left under it.
///
/// Its [`locale`](Self::locale) is what the page is answered in, and its
/// [`path_language`](Self::path_language) what its addresses are spelled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixMatch<'set, 'path> {
    prefix: &'set str,
    locale: &'set Locale,
    path_language: &'set str,
    scope: PrefixScope,
    rest: &'path str,
}

impl<'set, 'path> PrefixMatch<'set, 'path> {
    /// The matched entry and the remainder of the path.
    pub(super) const fn new(
        prefix: &'set str,
        locale: &'set Locale,
        path_language: &'set str,
        scope: PrefixScope,
        rest: &'path str,
    ) -> Self {
        Self {
            prefix,
            locale,
            path_language,
            scope,
            rest,
        }
    }

    /// The leading segment, with no slash.
    #[must_use]
    pub const fn prefix(&self) -> &'set str {
        self.prefix
    }

    /// The locale the page under this prefix is **answered in**.
    #[must_use]
    pub const fn locale(&self) -> &'set Locale {
        self.locale
    }

    /// The language this prefix's addresses are **spelled in**.
    ///
    /// A country's prefixes share one vocabulary whatever language they are read in; the
    /// locale's own language by default.
    #[must_use]
    pub const fn path_language(&self) -> &'set str {
        self.path_language
    }

    /// How much of the route tree this prefix serves.
    #[must_use]
    pub const fn scope(&self) -> PrefixScope {
        self.scope
    }

    /// The path under the prefix. Always begins with `/`, and is `"/"` for its root.
    #[must_use]
    pub const fn rest(&self) -> &'path str {
        self.rest
    }
}
