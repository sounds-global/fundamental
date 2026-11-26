//! [`RelationTuple`]: one relationship edge in the graph, backend-neutral.

use core::fmt;

use serde::{Deserialize, Serialize};

use super::{Caveat, ObjectRef, SubjectRef, TupleKey};
use crate::schema::RelationName;

/// A single relationship: `object#relation@subject`, optionally gated by a caveat.
///
/// The per-backend `#[derive(Model)]` rows convert to and from it. Renders in Zanzibar form.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelationTuple {
    /// The object the relation is on.
    pub object: ObjectRef,
    /// The relation.
    pub relation: RelationName,
    /// The subject holding the relation (a principal leaf or a userset).
    pub subject: SubjectRef,
    /// The caveat gating this edge, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caveat: Option<Caveat>,
}

impl RelationTuple {
    /// Builds an uncaveated tuple.
    #[must_use]
    pub fn new(object: ObjectRef, relation: RelationName, subject: SubjectRef) -> Self {
        Self {
            object,
            relation,
            subject,
            caveat: None,
        }
    }

    /// Returns this tuple gated by `caveat`.
    #[must_use]
    pub fn with_caveat(mut self, caveat: Caveat) -> Self {
        self.caveat = Some(caveat);
        self
    }

    /// The injective primary key of this tuple's identity; the caveat is not part of it.
    #[must_use]
    pub fn key(&self) -> TupleKey {
        TupleKey::of(self)
    }
}

impl fmt::Display for RelationTuple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}@{}", self.object, self.relation, self.subject)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{CaveatName, NamespaceName};
    use crate::tuple::{CanonicalId, CaveatContext};

    fn tuple() -> RelationTuple {
        RelationTuple::new(
            ObjectRef::new(
                NamespaceName::new("document").expect("valid"),
                CanonicalId::new("readme").expect("valid"),
            ),
            RelationName::new("viewer").expect("valid"),
            SubjectRef::leaf(
                NamespaceName::new("user").expect("valid"),
                CanonicalId::new("alice").expect("valid"),
            ),
        )
    }

    #[test]
    fn renders_canonical_form() {
        assert_eq!(tuple().to_string(), "document:readme#viewer@user:alice");
    }

    #[test]
    fn key_excludes_caveat() {
        let plain = tuple();
        let caveated = tuple().with_caveat(Caveat::new(
            CaveatName::new("not_expired").expect("valid"),
            CaveatContext::new(),
        ));
        assert_eq!(
            plain.key(),
            caveated.key(),
            "caveat is not part of identity"
        );
    }
}
