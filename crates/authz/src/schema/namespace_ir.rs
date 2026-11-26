//! [`NamespaceIr`]: the compiled definition of one object type.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{CaveatIr, CaveatName, NamespaceName, RelationIr, RelationName};

/// The compiled definition of one namespace (object type): its name, its
/// relations keyed by name, and the caveats those relations may reference.
///
/// `BTreeMap`s, so iteration and the schema hash are deterministic.
// Not `Eq`: caveats carry `f64` literals.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct NamespaceIr {
    /// The namespace's name.
    pub namespace: NamespaceName,
    /// The relations defined on this namespace, keyed by relation name.
    pub relations: BTreeMap<RelationName, RelationIr>,
    /// The caveats defined on this namespace, keyed by caveat name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub caveats: BTreeMap<CaveatName, CaveatIr>,
}

impl NamespaceIr {
    /// Builds a namespace from its name and relations, with no caveats.
    ///
    /// A later duplicate relation wins; [`SchemaBuilder::build`](super::SchemaBuilder) validates.
    #[must_use]
    pub fn new(namespace: NamespaceName, relations: Vec<RelationIr>) -> Self {
        let relations = relations.into_iter().map(|r| (r.name.clone(), r)).collect();
        Self {
            namespace,
            relations,
            caveats: BTreeMap::new(),
        }
    }

    /// Returns a copy of this namespace with `caveats` attached.
    #[must_use]
    pub fn with_caveats(mut self, caveats: BTreeMap<CaveatName, CaveatIr>) -> Self {
        self.caveats = caveats;
        self
    }

    /// Looks up a relation definition by name.
    #[must_use]
    pub fn relation(&self, name: &RelationName) -> Option<&RelationIr> {
        self.relations.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{RelatedType, RewriteIr};

    #[test]
    fn indexes_relations_by_name() {
        let owner = RelationName::new("owner").expect("valid");
        let ns = NamespaceIr::new(
            NamespaceName::new("document").expect("valid"),
            vec![RelationIr::new(
                owner.clone(),
                RewriteIr::This,
                vec![RelatedType::leaf(
                    NamespaceName::new("user").expect("valid"),
                )],
            )],
        );
        assert!(ns.relation(&owner).is_some());
        assert!(
            ns.relation(&RelationName::new("missing").expect("valid"))
                .is_none()
        );
    }
}
