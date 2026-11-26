//! [`PrefixScope`] — how much of the route tree a locale prefix serves.

/// How much of the route tree lives beneath a locale prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum PrefixScope {
    /// The whole route tree is served beneath this prefix.
    #[default]
    Full,

    /// Only the prefix's own root resolves; every deeper path permanently
    /// redirects to it: a locale recognised but not served.
    RootOnly,
}

impl PrefixScope {
    /// Whether the whole route tree is reachable beneath this prefix.
    #[must_use]
    pub const fn serves_every_page(self) -> bool {
        matches!(self, Self::Full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_is_the_default_because_a_prefix_normally_serves_the_site() {
        assert_eq!(PrefixScope::default(), PrefixScope::Full);
        assert!(PrefixScope::default().serves_every_page());
    }

    #[test]
    fn a_root_only_prefix_does_not_serve_every_page() {
        assert!(!PrefixScope::RootOnly.serves_every_page());
    }
}
