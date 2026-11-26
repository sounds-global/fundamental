//! [`SubjectRef`]: a subject, either a concrete principal or a userset.

use core::fmt;

use serde::{Deserialize, Serialize};

use super::{CanonicalId, ObjectRef};
use crate::schema::{NamespaceName, RelationName};

/// The right-hand side of a relation tuple — who (or what set) holds the relation.
///
/// A principal leaf (`user:alice`) has no relation; a userset (`group:eng#member`) has one.
/// A leaf with the [`CanonicalId::wildcard`] id is a public grant.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SubjectRef {
    /// The object naming the subject (or the userset's object).
    pub object: ObjectRef,
    /// `Some(relation)` for a userset; `None` for a principal leaf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<RelationName>,
}

impl SubjectRef {
    /// A principal-leaf subject, e.g. `user:alice`.
    #[must_use]
    pub fn leaf(namespace: NamespaceName, id: CanonicalId) -> Self {
        Self {
            object: ObjectRef::new(namespace, id),
            relation: None,
        }
    }

    /// A userset subject `namespace:id#relation`, e.g. `group:eng#member`.
    #[must_use]
    pub fn userset(namespace: NamespaceName, id: CanonicalId, relation: RelationName) -> Self {
        Self {
            object: ObjectRef::new(namespace, id),
            relation: Some(relation),
        }
    }

    /// A public/wildcard leaf `namespace:*`.
    #[must_use]
    pub fn wildcard(namespace: NamespaceName) -> Self {
        Self::leaf(namespace, CanonicalId::wildcard())
    }

    /// Whether this subject is a userset rather than a concrete principal.
    #[must_use]
    pub fn is_userset(&self) -> bool {
        self.relation.is_some()
    }

    /// Whether this subject is a public/wildcard leaf (`namespace:*`).
    #[must_use]
    pub fn is_wildcard(&self) -> bool {
        self.relation.is_none() && self.object.id.is_wildcard()
    }
}

impl fmt::Display for SubjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.relation {
            Some(relation) => write!(f, "{}#{relation}", self.object),
            None => write!(f, "{}", self.object),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(name: &str) -> NamespaceName {
        NamespaceName::new(name).expect("valid")
    }
    fn id(value: &str) -> CanonicalId {
        CanonicalId::new(value).expect("valid")
    }

    #[test]
    fn leaf_renders_without_relation() {
        let s = SubjectRef::leaf(ns("user"), id("alice"));
        assert_eq!(s.to_string(), "user:alice");
        assert!(!s.is_userset());
        assert!(!s.is_wildcard());
    }

    #[test]
    fn userset_renders_with_relation() {
        let s = SubjectRef::userset(
            ns("group"),
            id("eng"),
            RelationName::new("member").expect("valid"),
        );
        assert_eq!(s.to_string(), "group:eng#member");
        assert!(s.is_userset());
    }

    #[test]
    fn wildcard_is_a_public_leaf() {
        let s = SubjectRef::wildcard(ns("user"));
        assert_eq!(s.to_string(), "user:*");
        assert!(s.is_wildcard());
        assert!(!s.is_userset());
    }
}
