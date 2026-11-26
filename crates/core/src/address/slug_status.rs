//! [`SlugStatus`] — whether a declared slug is still an address or only a memory.

/// Whether a declared slug is the page's current address, or one it used to have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum SlugStatus {
    /// The page's address in this language: accepted inbound, and emitted outbound.
    #[default]
    Current,

    /// A spelling this page used to answer at.
    ///
    /// Accepted inbound and never emitted, so retiring a slug is a permanent redirect.
    Retired,
}

impl SlugStatus {
    /// Whether this spelling may be *emitted* — used to build a link, a canonical
    /// URL or an `hreflang` alternate.
    #[must_use]
    pub const fn is_current(self) -> bool {
        matches!(self, Self::Current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_slug_is_current_unless_it_says_otherwise() {
        assert_eq!(SlugStatus::default(), SlugStatus::Current);
        assert!(SlugStatus::default().is_current());
    }

    #[test]
    fn a_retired_slug_is_never_emitted() {
        assert!(!SlugStatus::Retired.is_current());
    }
}
