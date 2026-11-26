//! [`Ring`], one ring of the dependency rule.

use std::fmt;

/// One ring of the dependency rule, innermost first: a crate may depend inward, never outward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ring {
    /// The business's types and rules.
    Entities,
    /// One operation of the business each, owning the ports it needs.
    UseCases,
    /// The components a view lays its model out with.
    Components,
    /// Controllers, presenters, repositories and provider adapters.
    Adapters,
    /// The web, the database, the server, `main` and the tools around them.
    Drivers,
}

impl Ring {
    /// Every ring, innermost first.
    pub const ALL: [Self; 5] = [
        Self::Entities,
        Self::UseCases,
        Self::Components,
        Self::Adapters,
        Self::Drivers,
    ];

    /// The key that names this ring in `[architecture]`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Entities => "entities",
            Self::UseCases => "use_cases",
            Self::Components => "components",
            Self::Adapters => "adapters",
            Self::Drivers => "drivers",
        }
    }

    /// Whether a view (a page, a layout or a presenter) may name a crate in this ring.
    #[must_use]
    pub const fn open_to_views(self) -> bool {
        !matches!(self, Self::Adapters | Self::Drivers)
    }
}

impl fmt::Display for Ring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_order_innermost_first() {
        let mut sorted = Ring::ALL;
        sorted.sort();
        assert_eq!(sorted, Ring::ALL);
        assert!(Ring::Components < Ring::Adapters);
    }

    #[test]
    fn views_may_name_only_the_inner_rings_and_components() {
        let open: Vec<_> = Ring::ALL
            .into_iter()
            .filter(|r| r.open_to_views())
            .collect();
        assert_eq!(open, [Ring::Entities, Ring::UseCases, Ring::Components]);
    }

    #[test]
    fn a_ring_displays_as_its_key() {
        assert_eq!(Ring::UseCases.to_string(), "use_cases");
    }
}
