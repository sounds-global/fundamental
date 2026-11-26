//! [`SchemaIr`]: a fully compiled and validated authorization schema.

use std::collections::BTreeMap;

use serde::Serialize;
use tracing::{debug, instrument};

use super::refuse_locked_exclusion::refuse_locked_exclusion;
use super::{NamespaceIr, NamespaceName, RelationIr, RelationName, SchemaBuilder, SchemaHash};
use crate::error::AuthzError;

/// A fully compiled, validated authorization schema: every namespace, every
/// relation, and a stable [`SchemaHash`] over the whole.
///
/// Both front ends compile through [`compile`](SchemaIr::compile), which proves there are no
/// dangling references.
// Not `Eq`: caveat literals carry `f64`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SchemaIr {
    namespaces: BTreeMap<NamespaceName, NamespaceIr>,
    #[serde(skip)]
    schema_hash: SchemaHash,
}

impl SchemaIr {
    /// Starts building a schema from typed namespaces or namespace IR.
    pub fn builder() -> SchemaBuilder {
        SchemaBuilder::new()
    }

    /// Validates `namespaces` and compiles them into a schema with a stable content hash.
    ///
    /// # Errors
    ///
    /// Returns [`UndefinedNamespace`](AuthzError::UndefinedNamespace),
    /// [`UndefinedRelation`](AuthzError::UndefinedRelation) or
    /// [`UndefinedCaveat`](AuthzError::UndefinedCaveat) for the first dangling reference, and
    /// [`LockedRelationReachesExclusion`](AuthzError::LockedRelationReachesExclusion).
    #[instrument(skip(namespaces), fields(namespaces = namespaces.len()))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn compile(namespaces: BTreeMap<NamespaceName, NamespaceIr>) -> Result<Self, AuthzError> {
        for namespace in namespaces.values() {
            validate_namespace(namespace, &namespaces)?;
        }
        refuse_locked_exclusion(&namespaces)?;
        let schema_hash = fingerprint(&namespaces)?;
        debug!(%schema_hash, "compiled authorization schema");
        Ok(Self {
            namespaces,
            schema_hash,
        })
    }

    /// The stable fingerprint of this schema.
    #[must_use]
    pub fn hash(&self) -> SchemaHash {
        self.schema_hash
    }

    /// Looks up a namespace definition by name.
    #[must_use]
    pub fn namespace(&self, name: &NamespaceName) -> Option<&NamespaceIr> {
        self.namespaces.get(name)
    }

    /// Looks up a relation definition by namespace and relation name.
    #[must_use]
    pub fn relation(
        &self,
        namespace: &NamespaceName,
        relation: &RelationName,
    ) -> Option<&RelationIr> {
        self.namespaces.get(namespace)?.relation(relation)
    }

    /// The number of namespaces in the schema.
    #[must_use]
    pub fn len(&self) -> usize {
        self.namespaces.len()
    }

    /// Whether the schema defines no namespaces.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.namespaces.is_empty()
    }
}

#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn validate_namespace(
    namespace: &NamespaceIr,
    all: &BTreeMap<NamespaceName, NamespaceIr>,
) -> Result<(), AuthzError> {
    let ns_name = &namespace.namespace;

    for relation in namespace.relations.values() {
        let mut local = Vec::new();
        relation.rewrite.collect_local_relations(&mut local);
        for referenced in local {
            if !namespace.relations.contains_key(referenced) {
                return Err(AuthzError::UndefinedRelation {
                    namespace: ns_name.to_string(),
                    relation: referenced.to_string(),
                });
            }
        }

        let mut ttus = Vec::new();
        relation.rewrite.collect_tuple_to_usersets(&mut ttus);
        for (tupleset, computed) in ttus {
            let tupleset_rel = &namespace.relations[tupleset];
            for target in &tupleset_rel.directly_related_types {
                let target_ns =
                    all.get(&target.namespace)
                        .ok_or_else(|| AuthzError::UndefinedNamespace {
                            namespace: target.namespace.to_string(),
                        })?;
                if !target_ns.relations.contains_key(computed) {
                    return Err(AuthzError::UndefinedRelation {
                        namespace: target_ns.namespace.to_string(),
                        relation: computed.to_string(),
                    });
                }
            }
        }

        for restriction in &relation.directly_related_types {
            let target_ns =
                all.get(&restriction.namespace)
                    .ok_or_else(|| AuthzError::UndefinedNamespace {
                        namespace: restriction.namespace.to_string(),
                    })?;
            if let Some(subject_relation) = &restriction.relation
                && !target_ns.relations.contains_key(subject_relation)
            {
                return Err(AuthzError::UndefinedRelation {
                    namespace: target_ns.namespace.to_string(),
                    relation: subject_relation.to_string(),
                });
            }
            if let Some(caveat) = &restriction.caveat
                && !namespace.caveats.contains_key(caveat)
            {
                return Err(AuthzError::UndefinedCaveat {
                    namespace: ns_name.to_string(),
                    caveat: caveat.to_string(),
                });
            }
        }
    }

    Ok(())
}

/// Folds the schema's JSON form into a stable content hash, refusing a value it could not
/// serialise rather than hashing it as empty.
fn fingerprint<T: Serialize + ?Sized>(value: &T) -> Result<SchemaHash, AuthzError> {
    let bytes = serde_json::to_vec(value).map_err(|error| AuthzError::SchemaNotHashable {
        detail: error.to_string(),
    })?;
    Ok(SchemaHash::empty().update(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{RelatedType, RewriteIr};

    fn ns(name: &str) -> NamespaceName {
        NamespaceName::new(name).expect("valid namespace")
    }
    fn rel(name: &str) -> RelationName {
        RelationName::new(name).expect("valid relation")
    }

    /// document{ owner: [user], viewer: union(owner, parent->viewer), parent: [folder] }
    /// folder{ viewer: [user] }
    fn valid_schema() -> BTreeMap<NamespaceName, NamespaceIr> {
        let folder = NamespaceIr::new(
            ns("folder"),
            vec![RelationIr::direct(
                rel("viewer"),
                vec![RelatedType::leaf(ns("user"))],
            )],
        );
        let document = NamespaceIr::new(
            ns("document"),
            vec![
                RelationIr::direct(rel("owner"), vec![RelatedType::leaf(ns("user"))]),
                RelationIr::direct(rel("parent"), vec![RelatedType::leaf(ns("folder"))]),
                RelationIr::new(
                    rel("viewer"),
                    RewriteIr::Union(vec![
                        RewriteIr::ComputedUserset {
                            relation: rel("owner"),
                        },
                        RewriteIr::TupleToUserset {
                            tupleset: rel("parent"),
                            computed: rel("viewer"),
                        },
                    ]),
                    vec![RelatedType::leaf(ns("user"))],
                ),
            ],
        );
        let user = NamespaceIr::new(ns("user"), vec![]);
        BTreeMap::from([
            (ns("document"), document),
            (ns("folder"), folder),
            (ns("user"), user),
        ])
    }

    #[test]
    fn compiles_a_valid_schema_and_hashes_it() {
        let schema = SchemaIr::compile(valid_schema()).expect("valid schema compiles");
        assert_eq!(schema.len(), 3);
        assert!(schema.relation(&ns("document"), &rel("viewer")).is_some());

        let again = SchemaIr::compile(valid_schema()).expect("valid");
        assert_eq!(schema.hash(), again.hash());
    }

    #[test]
    fn rejects_computed_userset_to_missing_local_relation() {
        let mut doc = valid_schema();
        doc.get_mut(&ns("document")).unwrap().relations.insert(
            rel("editor"),
            RelationIr::new(
                rel("editor"),
                RewriteIr::ComputedUserset {
                    relation: rel("ghost"),
                },
                vec![],
            ),
        );
        let err = SchemaIr::compile(doc).expect_err("dangling local relation");
        match err {
            AuthzError::UndefinedRelation {
                namespace,
                relation,
            } => {
                assert_eq!(namespace, "document");
                assert_eq!(relation, "ghost");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn rejects_tuple_to_userset_missing_on_target_namespace() {
        let mut doc = valid_schema();
        if let RewriteIr::Union(children) = &mut doc
            .get_mut(&ns("document"))
            .unwrap()
            .relations
            .get_mut(&rel("viewer"))
            .unwrap()
            .rewrite
        {
            children[1] = RewriteIr::TupleToUserset {
                tupleset: rel("parent"),
                computed: rel("editor"),
            };
        }
        let err = SchemaIr::compile(doc).expect_err("computed missing on folder");
        match err {
            AuthzError::UndefinedRelation {
                namespace,
                relation,
            } => {
                assert_eq!(namespace, "folder");
                assert_eq!(relation, "editor");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn rejects_restriction_to_undefined_namespace() {
        let mut doc = valid_schema();
        doc.get_mut(&ns("document"))
            .unwrap()
            .relations
            .get_mut(&rel("owner"))
            .unwrap()
            .directly_related_types = vec![RelatedType::leaf(ns("ghost_ns"))];
        let err = SchemaIr::compile(doc).expect_err("undefined namespace");
        assert!(
            matches!(err, AuthzError::UndefinedNamespace { namespace } if namespace == "ghost_ns")
        );
    }

    #[test]
    fn a_schema_that_cannot_be_fingerprinted_is_an_error_not_the_empty_hash() {
        use fundamental_core::Error as _;

        struct Unserialisable;
        impl Serialize for Unserialisable {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("a value with no JSON form"))
            }
        }

        let error =
            fingerprint(&Unserialisable).expect_err("an unserialisable value has no fingerprint");
        assert!(
            matches!(error, AuthzError::SchemaNotHashable { .. }),
            "{error:?}"
        );
        assert_eq!(error.error_code(), "AUTHZ_018");
        assert_eq!(error.http_status_code(), 500);

        let compiled = SchemaIr::compile(valid_schema()).expect("a valid schema compiles");
        assert_ne!(compiled.hash(), SchemaHash::empty());
    }
}
