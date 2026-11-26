//! The object a request's guard authorized, as a type.

use std::fmt;
use std::marker::PhantomData;

use fundamental_server::{AuthorizedObjects, HttpContext};

use super::any_relation::AnyRelation;
use crate::schema::{Namespace, Relation, RelationOf};

/// The one object of namespace `N` this request's guard allowed at the relation `R`, by its
/// canonical id as checked.
///
/// Built only from what a guard recorded, never from the request itself. `R` defaults to
/// [`AnyRelation`].
pub struct Authorized<N, R = AnyRelation> {
    id: String,
    marker: PhantomData<fn() -> (N, R)>,
}

impl<N: Namespace> Authorized<N> {
    /// The object of `N` this request was authorized on, or `None` if no guard of `N` allowed
    /// one, or two allowed different ones.
    #[must_use]
    pub fn from_context<S>(ctx: &HttpContext<S>) -> Option<Self> {
        ctx.authorized_objects().only_in(N::NAME).map(Self::new)
    }
}

impl<N, R> Authorized<N, R>
where
    N: Namespace + Relation,
    R: RelationOf<N>,
{
    /// The object of `N` a guard allowed at exactly the relation `R`, or `None`.
    #[must_use]
    pub fn granted_in(objects: &AuthorizedObjects) -> Option<Self> {
        objects.granted(N::NAME, R::relation_name()).map(Self::new)
    }
}

impl<N, R> Authorized<N, R> {
    fn new(id: String) -> Self {
        Self {
            id,
            marker: PhantomData,
        }
    }

    /// The object's canonical id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl<N: Namespace, R> fmt::Debug for Authorized<N, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Authorized<{}>({})", N::NAME, self.id)
    }
}
