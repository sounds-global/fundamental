//! [`RelationIr`]: the compiled definition of one relation.

use serde::{Deserialize, Serialize};

use super::is_false::is_false;

use super::{RelatedType, RelationName, RewriteIr};

/// The compiled definition of a single relation on a namespace: its name, how its
/// effective subject set is computed ([`RewriteIr`]), and the subject shapes that
/// may be stored on it directly ([`RelatedType`]).
///
/// `directly_related_types` also resolves a [`RewriteIr::TupleToUserset`]'s hop targets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RelationIr {
    /// The relation's name (unique within its namespace).
    pub name: RelationName,
    /// How the relation's effective subject set is computed.
    pub rewrite: RewriteIr,
    /// The subject shapes that may be stored directly on this relation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directly_related_types: Vec<RelatedType>,
    /// Whether the relation may be checked under locks, inside an act's own transaction.
    ///
    /// Such a relation must not reach a [`RewriteIr::Exclusion`]. Left out of the serialised form
    /// when unset, so the hash is unchanged.
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
}

impl RelationIr {
    /// Builds a relation definition.
    #[must_use]
    pub fn new(
        name: RelationName,
        rewrite: RewriteIr,
        directly_related_types: Vec<RelatedType>,
    ) -> Self {
        Self {
            name,
            rewrite,
            directly_related_types,
            locked: false,
        }
    }

    /// Returns this relation declared for locked checks.
    #[must_use]
    pub fn with_lock(mut self) -> Self {
        self.locked = true;
        self
    }

    /// A relation whose subjects are only those stored directly on it
    /// (`rewrite = This`), restricted to `types`.
    #[must_use]
    pub fn direct(name: RelationName, types: Vec<RelatedType>) -> Self {
        Self::new(name, RewriteIr::This, types)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::NamespaceName;

    #[test]
    fn direct_relation_has_this_rewrite() {
        let r = RelationIr::direct(
            RelationName::new("owner").expect("valid"),
            vec![RelatedType::leaf(
                NamespaceName::new("user").expect("valid"),
            )],
        );
        assert_eq!(r.rewrite, RewriteIr::This);
        assert_eq!(r.directly_related_types.len(), 1);
    }

    #[test]
    fn a_relation_is_unlocked_until_declared() {
        let user = vec![RelatedType::leaf(
            NamespaceName::new("user").expect("valid"),
        )];
        let plain = RelationIr::direct(RelationName::new("owner").expect("valid"), user);
        assert!(!plain.locked);
        assert!(plain.with_lock().locked);
    }

    #[test]
    fn an_unlocked_relation_serialises_without_the_flag() {
        let user = vec![RelatedType::leaf(
            NamespaceName::new("user").expect("valid"),
        )];
        let plain = RelationIr::direct(RelationName::new("owner").expect("valid"), user);
        let json = serde_json::to_string(&plain).expect("serialises");
        assert!(!json.contains("locked"), "{json}");

        let locked = serde_json::to_string(&plain.with_lock()).expect("serialises");
        assert!(locked.contains("\"locked\":true"), "{locked}");
        let back: RelationIr = serde_json::from_str(&locked).expect("deserialises");
        assert!(back.locked);
    }
}
