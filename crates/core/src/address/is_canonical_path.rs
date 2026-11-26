//! [`is_canonical_path`] — whether a path is a page's one spelling.

use crate::address::canonical_spelling::canonical_spelling;

/// Whether `path` is written the one way an address is: a leading `/`, no empty segment,
/// no trailing `/` (except the root), every percent-escape in upper case, and no query,
/// fragment, whitespace or control character.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::is_canonical_path;
///
/// assert!(is_canonical_path("/prijzen"));
/// assert!(!is_canonical_path("//prijzen"));
/// assert!(!is_canonical_path("/a%2f"));
/// ```
#[must_use]
pub fn is_canonical_path(path: &str) -> bool {
    path.starts_with('/')
        && (path == "/" || !path.ends_with('/'))
        && !path
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, '?' | '#'))
        && canonical_spelling(path) == path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_one_spelling_is_canonical() {
        for path in [
            "/",
            "/prijzen",
            "/en/prijzen",
            "/a%2Fb",
            "/gidsen/b%C3%BCcher",
        ] {
            assert!(is_canonical_path(path), "{path}");
        }
        for path in [
            "",
            "prijzen",
            "//prijzen",
            "/a//b",
            "/a/",
            "/a%2fb",
            "/a?b",
            "/a#b",
            "/a b",
            "/a\nb",
        ] {
            assert!(!is_canonical_path(path), "{path}");
        }
    }
}
