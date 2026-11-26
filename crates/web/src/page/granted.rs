use fundamental_authz::middleware::Authorized;
use fundamental_authz::schema::{Namespace, Relation, RelationOf};
use fundamental_server::AuthorizedObjects;

use super::requirement::Requirement;
use super::sealed::Sealed;

/// An argument a loader takes: an object a guard granted on this request.
///
/// Sealed, implemented only for `Authorized<N, R>`, so a loader has no other way in.
pub trait Granted: Sealed + Sized + Send + 'static {
    /// The object this argument needs a guard to have granted.
    fn requirement() -> Requirement;

    /// The granted object, or `None` when no guard granted it on this request.
    fn take(objects: &AuthorizedObjects) -> Option<Self>;
}

impl<N, R> Sealed for Authorized<N, R>
where
    N: Namespace + Relation + 'static,
    R: RelationOf<N>,
{
}

impl<N, R> Granted for Authorized<N, R>
where
    N: Namespace + Relation + 'static,
    R: RelationOf<N>,
{
    fn requirement() -> Requirement {
        Requirement {
            namespace: N::NAME,
            relation: R::relation_name(),
        }
    }

    fn take(objects: &AuthorizedObjects) -> Option<Self> {
        Self::granted_in(objects)
    }
}
