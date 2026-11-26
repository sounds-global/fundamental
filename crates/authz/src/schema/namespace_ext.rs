//! [`NamespaceExt`]: object and subject constructors on a namespace type.

use super::{Namespace, NamespaceName};
use crate::error::AuthzError;
use crate::tuple::{CanonicalId, IntoCanonicalId, ObjectRef, SubjectRef};

/// Fluent constructors for the object and subject references of a
/// [`#[derive(Namespace)]`](Namespace) type, such as `Document::object("readme")`.
pub trait NamespaceExt: Namespace {
    /// This namespace's validated name.
    ///
    /// # Panics
    ///
    /// Never through `#[derive(Namespace)]`, which guarantees [`NAME`](Namespace::NAME) is valid.
    #[must_use]
    fn name() -> NamespaceName;

    /// The object `namespace:id` (e.g. `document:readme`).
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::InvalidCanonicalId`] if `id` is not a safe identifier.
    fn object(id: impl IntoCanonicalId) -> Result<ObjectRef, AuthzError>;

    /// The leaf subject `namespace:id` (e.g. `user:alice`).
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::InvalidCanonicalId`] if `id` is not a safe identifier.
    fn subject(id: impl IntoCanonicalId) -> Result<SubjectRef, AuthzError>;

    /// The public wildcard subject `namespace:*` — every subject of this namespace.
    #[must_use]
    fn wildcard() -> SubjectRef;

    /// `id` reduced to this namespace's one spelling; see [`Namespace::canonical_id`].
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::InvalidCanonicalId`] if `id` is not a safe identifier,
    /// or is not a well-formed id of this namespace.
    fn canonical(id: impl IntoCanonicalId) -> Result<CanonicalId, AuthzError>;
}

impl<N: Namespace> NamespaceExt for N {
    fn canonical(id: impl IntoCanonicalId) -> Result<CanonicalId, AuthzError> {
        let id = id.into_canonical_id()?;
        let canonical =
            N::canonical_id(id.as_str()).ok_or_else(|| AuthzError::InvalidCanonicalId {
                value: id.as_str().chars().take(64).collect(),
            })?;
        CanonicalId::new(canonical)
    }

    fn name() -> NamespaceName {
        NamespaceName::new(N::NAME).expect("derive guarantees a valid namespace identifier")
    }

    fn object(id: impl IntoCanonicalId) -> Result<ObjectRef, AuthzError> {
        // Through the namespace's canonical form, so a written tuple and a guard's check agree.
        Ok(ObjectRef::new(Self::name(), Self::canonical(id)?))
    }

    fn subject(id: impl IntoCanonicalId) -> Result<SubjectRef, AuthzError> {
        // The same form as `object`: one namespace is often both.
        Ok(SubjectRef::leaf(Self::name(), Self::canonical(id)?))
    }

    fn wildcard() -> SubjectRef {
        SubjectRef::wildcard(Self::name())
    }
}
