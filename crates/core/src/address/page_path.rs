//! [`PagePath`] — a page's canonical address, printable only through an address space.

#[cfg(not(feature = "std"))]
use alloc::{borrow::Cow, string::String};
#[cfg(feature = "std")]
use std::borrow::Cow;

use crate::address::percent_encode_segment;

/// A page's canonical address — its route path, with the query and fragment a link carries
/// kept apart from it.
///
/// It prints only through [`RequestAddress::href_to`](crate::address::RequestAddress::href_to)
/// and the [`AddressSpace`](crate::address::AddressSpace)'s `page_*` builders, which spell it
/// for the reader.
///
/// ```compile_fail
/// use fundamental_core::address::PagePath;
///
/// let page = PagePath::of_route("/aanmelden");
/// // A page path has no `Display`: it is printed by an address space.
/// let href = format!("{page}");
/// ```
///
/// ```compile_fail
/// use fundamental_core::address::PagePath;
///
/// // Nor any conversion to a string.
/// let href: String = PagePath::of_route("/aanmelden").into();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PagePath {
    route: Cow<'static, str>,
    query: String,
    fragment: Option<String>,
    directory: bool,
}

impl PagePath {
    /// The page at a route path without captures.
    ///
    /// Called by the generated route builders only.
    #[doc(hidden)]
    #[must_use]
    pub const fn of_route(route: &'static str) -> Self {
        Self {
            route: Cow::Borrowed(route),
            query: String::new(),
            fragment: None,
            directory: false,
        }
    }

    /// The page at a route path whose captures the generated builder filled in, each one
    /// escaped by [`PathSegment`](crate::address::PathSegment).
    #[doc(hidden)]
    #[must_use]
    pub fn of_built_route(route: String) -> Self {
        Self {
            route: Cow::Owned(route),
            query: String::new(),
            fragment: None,
            directory: false,
        }
    }

    /// The same page with `name=value` appended to its query, each escaped so that neither
    /// can end the pair or the query: `/` in a return address arrives as `%2F`.
    #[must_use]
    pub fn with_query(mut self, name: &str, value: &str) -> Self {
        if !self.query.is_empty() {
            self.query.push('&');
        }
        self.query.push_str(&percent_encode_segment(name));
        self.query.push('=');
        self.query.push_str(&percent_encode_segment(value));
        self
    }

    /// The same page, scrolled to the element `id` names.
    #[must_use]
    pub fn with_fragment(mut self, id: &str) -> Self {
        self.fragment = Some(percent_encode_segment(id).into_owned());
        self
    }

    /// The same page, its path spelled as a directory: ending in `/` (`/portfolio/acme/`),
    /// before any query.
    #[must_use]
    pub const fn as_directory(mut self) -> Self {
        self.directory = true;
        self
    }

    /// Whether `other` is the same page, whatever query or fragment either carries.
    #[must_use]
    pub fn same_page(&self, other: &Self) -> bool {
        self.route == other.route
    }

    /// Whether this is the page at `canonical_path`, a path an address space
    /// [resolved](crate::address::AddressSpace::resolve) to its canonical spelling, whatever
    /// query or fragment this carries.
    #[must_use]
    pub fn is_at(&self, canonical_path: &str) -> bool {
        self.route == canonical_path
    }

    /// The route path, canonical.
    pub(crate) fn route(&self) -> &str {
        &self.route
    }

    /// `path`, this page's route as an address space spelled it, followed by what this page adds
    /// to it: the directory's `/`, then the query and the fragment.
    pub(crate) fn completed(&self, mut path: String) -> String {
        if self.directory && !path.ends_with('/') {
            path.push('/');
        }
        path.push_str(&self.suffix());
        path
    }

    /// The `?query#fragment` that follows the path once it is spelled, or nothing.
    fn suffix(&self) -> String {
        let mut suffix = String::new();
        if !self.query.is_empty() {
            suffix.push('?');
            suffix.push_str(&self.query);
        }
        if let Some(fragment) = &self.fragment {
            suffix.push('#');
            suffix.push_str(fragment);
        }
        suffix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_at_its_canonical_path_whatever_it_carries() {
        let page = PagePath::of_route("/account/adres")
            .with_query("y", "z")
            .with_fragment("form");
        assert!(page.is_at("/account/adres"));
        assert!(!page.is_at("/account"));
        assert!(!page.is_at("/account/adres?y=z"));
    }

    #[test]
    fn a_query_value_cannot_end_its_pair_or_the_query() {
        let page = PagePath::of_route("/aanmelden")
            .with_query("next", "/a?b=c&d#e")
            .with_query("als", "acme");
        assert_eq!(page.suffix(), "?next=%2Fa%3Fb%3Dc%26d%23e&als=acme");
        assert_eq!(page.route(), "/aanmelden");
    }

    #[test]
    fn a_fragment_follows_the_query() {
        let page = PagePath::of_route("/x")
            .with_fragment("blok 2")
            .with_query("deel", "1");
        assert_eq!(page.suffix(), "?deel=1#blok%202");
    }

    #[test]
    fn the_same_page_ignores_what_a_link_adds() {
        let bare = PagePath::of_route("/x");
        let linked = PagePath::of_built_route("/x".into()).with_fragment("top");
        assert!(bare.same_page(&linked));
        assert_ne!(bare, linked);
        assert!(!bare.same_page(&PagePath::of_route("/y")));
    }
}
