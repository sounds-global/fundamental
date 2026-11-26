//! [`RouteKind`], the matching class of a discovered route.

/// How a route's URL pattern matches incoming paths; routes sort static, dynamic, catch-all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RouteKind {
    /// A fully literal path, e.g. `/about` (highest match priority).
    Static,
    /// A path with one or more `{param}` segments, e.g. `/users/{id}`.
    ///
    /// Including a literal filename below a dynamic directory, such as `pages/[org]/settings.rs`.
    Dynamic,
    /// A trailing `{*param}` that captures the remaining segments, e.g. `/blog/{*slug}`.
    CatchAll,
}

impl RouteKind {
    pub(crate) fn priority(self) -> u8 {
        match self {
            Self::Static => 0,
            Self::Dynamic => 1,
            Self::CatchAll => 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_orders_static_dynamic_catch_all() {
        assert!(RouteKind::Static.priority() < RouteKind::Dynamic.priority());
        assert!(RouteKind::Dynamic.priority() < RouteKind::CatchAll.priority());
    }
}
