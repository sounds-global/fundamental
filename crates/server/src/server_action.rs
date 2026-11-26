use fundamental_core::action::Action;

use crate::Route;

/// The server-side half of an [`Action`]: the builder for the [`Route`] that serves it over `S`.
///
/// A handler over a concrete state is projected through [`FromRef`](crate::FromRef); one over
/// a generic state is built for `S` itself.
pub trait ServerAction<S>: Action {
    /// Builds the [`Route`] that serves this action on a router over `S`.
    fn route() -> Route<S>;
}
