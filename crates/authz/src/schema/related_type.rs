//! [`RelatedType`]: one allowed subject shape for a direct relation.

use serde::{Deserialize, Serialize};

use super::is_false::is_false;

use super::{CaveatName, NamespaceName, RelationName};

/// One shape of subject that may be stored directly on a relation: a leaf (`user`), a
/// userset (`group#member`), a wildcard (`user:*`), optionally gated on a caveat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RelatedType {
    /// The namespace of the allowed subject.
    pub namespace: NamespaceName,
    /// The subject relation for a userset restriction, or `None` for a leaf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<RelationName>,
    /// Whether a `namespace:*` public/wildcard grant is allowed for this shape.
    #[serde(default, skip_serializing_if = "is_false")]
    pub wildcard: bool,
    /// The caveat this direct edge is gated on, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caveat: Option<CaveatName>,
}

impl RelatedType {
    /// A subject-leaf restriction of type `namespace` (e.g. `user`).
    #[must_use]
    pub fn leaf(namespace: NamespaceName) -> Self {
        Self {
            namespace,
            relation: None,
            wildcard: false,
            caveat: None,
        }
    }

    /// A userset restriction `namespace#relation` (e.g. `group#member`).
    #[must_use]
    pub fn userset(namespace: NamespaceName, relation: RelationName) -> Self {
        Self {
            namespace,
            relation: Some(relation),
            wildcard: false,
            caveat: None,
        }
    }

    /// Returns a copy of this restriction marked as allowing a wildcard grant.
    #[must_use]
    pub fn with_wildcard(mut self) -> Self {
        self.wildcard = true;
        self
    }

    /// Returns a copy of this restriction gated on `caveat`.
    #[must_use]
    pub fn with_caveat(mut self, caveat: CaveatName) -> Self {
        self.caveat = Some(caveat);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(name: &str) -> NamespaceName {
        NamespaceName::new(name).expect("valid namespace")
    }

    fn rel(name: &str) -> RelationName {
        RelationName::new(name).expect("valid relation")
    }

    #[test]
    fn constructs_leaf_and_userset() {
        let leaf = RelatedType::leaf(ns("user"));
        assert_eq!(leaf.namespace, ns("user"));
        assert!(leaf.relation.is_none());

        let set = RelatedType::userset(ns("group"), rel("member"));
        assert_eq!(set.relation, Some(rel("member")));
    }

    #[test]
    fn wildcard_and_caveat_builders() {
        let t = RelatedType::leaf(ns("user"))
            .with_wildcard()
            .with_caveat(CaveatName::new("non_expired").expect("valid"));
        assert!(t.wildcard);
        assert_eq!(
            t.caveat.as_ref().map(CaveatName::as_str),
            Some("non_expired")
        );
    }
}
