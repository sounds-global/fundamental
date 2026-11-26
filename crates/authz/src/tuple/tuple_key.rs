//! [`TupleKey`]: the injective single-column primary key for a relation tuple.

use core::fmt;

use serde::{Deserialize, Serialize};

use super::RelationTuple;

/// An injective, length-prefixed encoding of a tuple's six identity components, the
/// single-column key on the relational and offline backends. Never parsed back.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TupleKey(String);

impl TupleKey {
    /// Encodes the six identity components into a key.
    #[must_use]
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub fn from_parts(
        object_ns: &str,
        object_id: &str,
        relation: &str,
        subject_ns: &str,
        subject_id: &str,
        subject_relation: &str,
    ) -> Self {
        let parts = [
            object_ns,
            object_id,
            relation,
            subject_ns,
            subject_id,
            subject_relation,
        ];
        let mut key = String::new();
        for part in parts {
            key.push_str(&part.len().to_string());
            key.push(':');
            key.push_str(part);
        }
        Self(key)
    }

    /// Encodes the identity of a logical tuple; the subject relation is empty for a leaf.
    #[must_use]
    pub fn of(tuple: &RelationTuple) -> Self {
        let subject_relation = tuple
            .subject
            .relation
            .as_ref()
            .map(|r| r.as_str())
            .unwrap_or("");
        Self::from_parts(
            tuple.object.namespace.as_str(),
            tuple.object.id.as_str(),
            tuple.relation.as_str(),
            tuple.subject.object.namespace.as_str(),
            tuple.subject.object.id.as_str(),
            subject_relation,
        )
    }

    /// Returns the key as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TupleKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_injective_across_boundary_shifts() {
        let a = TupleKey::from_parts("ab", "c", "r", "s", "i", "");
        let b = TupleKey::from_parts("a", "bc", "r", "s", "i", "");
        assert_ne!(
            a, b,
            "length-prefixing must prevent boundary-shift collisions"
        );
    }

    #[test]
    fn is_deterministic() {
        let a = TupleKey::from_parts("document", "readme", "viewer", "user", "alice", "");
        let b = TupleKey::from_parts("document", "readme", "viewer", "user", "alice", "");
        assert_eq!(a, b);
    }

    #[test]
    fn distinguishes_leaf_from_userset_subject() {
        let leaf = TupleKey::from_parts("doc", "1", "viewer", "group", "eng", "");
        let userset = TupleKey::from_parts("doc", "1", "viewer", "group", "eng", "member");
        assert_ne!(leaf, userset);
    }
}
