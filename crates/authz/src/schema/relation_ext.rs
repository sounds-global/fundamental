//! [`RelationExt`]: build a userset subject from a typed relation value.

use super::{Namespace, NamespaceExt, Relation, RelationName};
use crate::error::AuthzError;
use crate::tuple::{IntoCanonicalId, SubjectRef};

/// Casts a typed relation value into a userset subject: `Group::Member.userset("eng")` is
/// `group:eng#member`.
pub trait RelationExt: Namespace + Relation {
    /// The userset subject `namespace:id#relation`.
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::InvalidCanonicalId`] if `id` is not a safe identifier.
    fn userset(&self, id: impl IntoCanonicalId) -> Result<SubjectRef, AuthzError>;
}

impl<T: Namespace + Relation> RelationExt for T {
    fn userset(&self, id: impl IntoCanonicalId) -> Result<SubjectRef, AuthzError> {
        Ok(SubjectRef::userset(
            Self::name(),
            id.into_canonical_id()?,
            RelationName::new(self.relation_name())
                .expect("derive guarantees a valid relation identifier"),
        ))
    }
}
