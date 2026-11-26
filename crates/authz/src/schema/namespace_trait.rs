//! [`Namespace`]: the trait a `#[derive(Namespace)]` object type implements.

use std::borrow::Cow;

use super::{NamespaceIr, Relation};

/// A typed object type (Zanzibar namespace).
///
/// The compile-time handle; [`ir`](Namespace::ir) gives the runtime [`NamespaceIr`].
pub trait Namespace {
    /// The namespace's name (e.g. `"document"`), a safe identifier.
    const NAME: &'static str;

    /// This namespace's relation enum.
    type Relation: Relation;

    /// The compiled definition of this namespace.
    fn ir() -> NamespaceIr;

    /// Reduces `raw` to this namespace's one spelling of an id, or `None` if it is
    /// not a well-formed id of this namespace.
    ///
    /// Governs objects and subjects alike, since the store cannot know two strings name one
    /// object. Returning `None` must deny. The default is the identity; set another with
    /// `#[namespace(canonical_id = path::to::fn)]`, such as [`uuid_id`](super::uuid_id).
    fn canonical_id(raw: &str) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(raw))
    }
}
