//! [`Landing`] — the control a reply lands at.

#[cfg(not(feature = "std"))]
use alloc::{format, string::String};

/// The longest anchor a landing names: the longest element id a form's control is given.
const LIMIT: usize = 64;

/// The control a reply lands at, named by its element id, so the reader carries on where they
/// pressed.
///
/// Carried as a fragment and as the query `at`; only small letters, digits and `_` are
/// accepted, so a planted `?at=` can name an element but never inject one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Landing(String);

impl Landing {
    /// The query a landing is written in and read from.
    pub const QUERY: &'static str = "at";

    /// The landing at the element `anchor` names; `None` when `anchor` is not an element id of
    /// small letters, digits and `_`, one to 64 of them.
    #[must_use]
    pub fn new(anchor: &str) -> Option<Self> {
        let fits = (1..=LIMIT).contains(&anchor.len())
            && anchor
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        fits.then(|| Self(anchor.into()))
    }

    /// The landing a raw query names in `at`; `None` when it names none, or names something
    /// [`new`](Self::new) refuses.
    #[must_use]
    pub fn read(query: &str) -> Option<Self> {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(Self::QUERY)?.strip_prefix('='))
            .and_then(Self::new)
    }

    /// The element id it lands at.
    #[must_use]
    pub fn anchor(&self) -> &str {
        &self.0
    }

    /// `page` with this landing added: its query keeps what it held and gains `at`, and its
    /// fragment, if it had one, becomes this landing's.
    #[must_use]
    pub fn location(&self, page: &str) -> String {
        let page = page.split_once('#').map_or(page, |(path, _)| path);
        let joiner = if page.contains('?') { '&' } else { '?' };
        format!("{page}{joiner}{}={}#{}", Self::QUERY, self.0, self.0)
    }
}
