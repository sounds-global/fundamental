//! [`refuse_locked_exclusion`]: a locked relation must never reach an exclusion.

use std::collections::{BTreeMap, VecDeque};

use super::{NamespaceIr, NamespaceName, RelationName, RewriteIr};
use crate::error::AuthzError;

type Node = (NamespaceName, RelationName);

/// Refuses a schema in which a relation declared `#[locked]` can reach an
/// [`RewriteIr::Exclusion`].
///
/// The reach mirrors what the engine walks: stored usersets, computed usersets and
/// tuple-to-userset hops, but not a tupleset's own rewrite. Caveats do not widen it.
///
/// # Errors
///
/// [`AuthzError::LockedRelationReachesExclusion`] for the first locked relation, in name
/// order, that reaches an exclusion — naming it, the exclusion, and the path between.
pub(crate) fn refuse_locked_exclusion(
    all: &BTreeMap<NamespaceName, NamespaceIr>,
) -> Result<(), AuthzError> {
    for namespace in all.values() {
        for relation in namespace.relations.values().filter(|r| r.locked) {
            let start = (namespace.namespace.clone(), relation.name.clone());
            if let Some(path) = path_to_exclusion(all, &start) {
                return Err(AuthzError::LockedRelationReachesExclusion {
                    namespace: start.0.to_string(),
                    relation: start.1.to_string(),
                    exclusion: label(path.last().unwrap_or(&start)),
                    path: path.iter().map(label).collect::<Vec<_>>().join(" -> "),
                });
            }
        }
    }
    Ok(())
}

/// The shortest chain of relations from `start` to one whose rewrite holds an exclusion,
/// both ends included; breadth first, so a cyclic schema terminates.
fn path_to_exclusion(
    all: &BTreeMap<NamespaceName, NamespaceIr>,
    start: &Node,
) -> Option<Vec<Node>> {
    let mut parent: BTreeMap<Node, Option<Node>> = BTreeMap::from([(start.clone(), None)]);
    let mut queue = VecDeque::from([start.clone()]);
    while let Some(node) = queue.pop_front() {
        let Some(namespace) = all.get(&node.0) else {
            continue;
        };
        let Some(relation) = namespace.relation(&node.1) else {
            continue;
        };
        let mut next = Vec::new();
        if reaches(
            &relation.rewrite,
            namespace,
            relation_types(namespace, &node.1),
            &mut next,
        ) {
            return Some(chain(&parent, node));
        }
        for reached in next {
            if !parent.contains_key(&reached) {
                parent.insert(reached.clone(), Some(node.clone()));
                queue.push_back(reached);
            }
        }
    }
    None
}

/// Collects the nodes `rewrite` hands evaluation to, and whether it holds an exclusion.
fn reaches(
    rewrite: &RewriteIr,
    namespace: &NamespaceIr,
    own_types: &[super::RelatedType],
    next: &mut Vec<Node>,
) -> bool {
    match rewrite {
        RewriteIr::This => {
            next.extend(own_types.iter().filter_map(|t| {
                t.relation
                    .as_ref()
                    .map(|subject_relation| (t.namespace.clone(), subject_relation.clone()))
            }));
            false
        }
        RewriteIr::ComputedUserset { relation } => {
            next.push((namespace.namespace.clone(), relation.clone()));
            false
        }
        RewriteIr::TupleToUserset { tupleset, computed } => {
            next.extend(
                relation_types(namespace, tupleset)
                    .iter()
                    .map(|t| (t.namespace.clone(), computed.clone())),
            );
            false
        }
        RewriteIr::Union(children) | RewriteIr::Intersection(children) => {
            // Every child is visited, so each one's reach is collected.
            let mut found = false;
            for child in children {
                found |= reaches(child, namespace, own_types, next);
            }
            found
        }
        RewriteIr::Exclusion { .. } => true,
    }
}

fn relation_types<'a>(
    namespace: &'a NamespaceIr,
    relation: &RelationName,
) -> &'a [super::RelatedType] {
    namespace
        .relation(relation)
        .map_or(&[], |r| r.directly_related_types.as_slice())
}

fn chain(parent: &BTreeMap<Node, Option<Node>>, end: Node) -> Vec<Node> {
    let mut path = vec![end];
    while let Some(Some(previous)) = path.last().and_then(|node| parent.get(node)) {
        path.push(previous.clone());
    }
    path.reverse();
    path
}

fn label(node: &Node) -> String {
    format!("{}#{}", node.0, node.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{RelatedType, RelationIr};
    use fundamental_core::Error as _;

    fn ns(name: &str) -> NamespaceName {
        NamespaceName::new(name).expect("valid namespace")
    }
    fn rel(name: &str) -> RelationName {
        RelationName::new(name).expect("valid relation")
    }
    fn users() -> Vec<RelatedType> {
        vec![RelatedType::leaf(ns("user"))]
    }
    fn schema(namespaces: Vec<NamespaceIr>) -> BTreeMap<NamespaceName, NamespaceIr> {
        let mut all: BTreeMap<_, _> = namespaces
            .into_iter()
            .map(|n| (n.namespace.clone(), n))
            .collect();
        all.insert(ns("user"), NamespaceIr::new(ns("user"), vec![]));
        all
    }
    fn but_not(include: &str, exclude: &str) -> RewriteIr {
        RewriteIr::Exclusion {
            include: Box::new(RewriteIr::ComputedUserset {
                relation: rel(include),
            }),
            exclude: Box::new(RewriteIr::ComputedUserset {
                relation: rel(exclude),
            }),
        }
    }

    /// team { member: [user], banned: [user], active: member but not banned }
    fn team(active_locked: bool) -> NamespaceIr {
        let active = RelationIr::new(rel("active"), but_not("member", "banned"), vec![]);
        NamespaceIr::new(
            ns("team"),
            vec![
                RelationIr::direct(rel("member"), users()),
                RelationIr::direct(rel("banned"), users()),
                if active_locked {
                    active.with_lock()
                } else {
                    active
                },
            ],
        )
    }

    fn refused(all: &BTreeMap<NamespaceName, NamespaceIr>) -> (String, String, String, String) {
        let error =
            refuse_locked_exclusion(all).expect_err("a locked relation reaches an exclusion");
        assert_eq!(error.error_code(), "AUTHZ_021");
        assert_eq!(error.http_status_code(), 500);
        match error {
            AuthzError::LockedRelationReachesExclusion {
                namespace,
                relation,
                exclusion,
                path,
            } => (namespace, relation, exclusion, path),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn an_unlocked_exclusion_is_admitted() {
        assert!(refuse_locked_exclusion(&schema(vec![team(false)])).is_ok());
    }

    #[test]
    fn a_locked_monotone_schema_is_admitted() {
        let doc = NamespaceIr::new(
            ns("document"),
            vec![
                RelationIr::direct(rel("owner"), users()),
                RelationIr::new(
                    rel("editor"),
                    RewriteIr::Intersection(vec![
                        RewriteIr::ComputedUserset {
                            relation: rel("owner"),
                        },
                        RewriteIr::This,
                    ]),
                    users(),
                )
                .with_lock(),
            ],
        );
        assert!(refuse_locked_exclusion(&schema(vec![doc])).is_ok());
    }

    #[test]
    fn a_locked_relation_holding_an_exclusion_is_refused() {
        let (namespace, relation, exclusion, path) = refused(&schema(vec![team(true)]));
        assert_eq!((namespace.as_str(), relation.as_str()), ("team", "active"));
        assert_eq!(exclusion, "team#active");
        assert_eq!(path, "team#active");
    }

    #[test]
    fn an_exclusion_through_a_computed_userset_is_refused() {
        let mut team = team(false);
        team.relations.insert(
            rel("acts"),
            RelationIr::new(
                rel("acts"),
                RewriteIr::Union(vec![
                    RewriteIr::This,
                    RewriteIr::ComputedUserset {
                        relation: rel("active"),
                    },
                ]),
                users(),
            )
            .with_lock(),
        );
        let (_, relation, exclusion, path) = refused(&schema(vec![team]));
        assert_eq!(relation, "acts");
        assert_eq!(exclusion, "team#active");
        assert_eq!(path, "team#acts -> team#active");
    }

    #[test]
    fn an_exclusion_through_a_userset_type_is_refused() {
        let doc = NamespaceIr::new(
            ns("document"),
            vec![
                RelationIr::direct(
                    rel("editor"),
                    vec![RelatedType::userset(ns("team"), rel("active"))],
                )
                .with_lock(),
            ],
        );
        let (namespace, _, exclusion, path) = refused(&schema(vec![doc, team(false)]));
        assert_eq!(namespace, "document");
        assert_eq!(exclusion, "team#active");
        assert_eq!(path, "document#editor -> team#active");
    }

    #[test]
    fn an_exclusion_through_a_tuple_to_userset_hop_is_refused() {
        let doc = NamespaceIr::new(
            ns("document"),
            vec![
                RelationIr::direct(rel("owner_team"), vec![RelatedType::leaf(ns("team"))]),
                RelationIr::new(
                    rel("editor"),
                    RewriteIr::TupleToUserset {
                        tupleset: rel("owner_team"),
                        computed: rel("active"),
                    },
                    vec![],
                )
                .with_lock(),
            ],
        );
        let (_, _, exclusion, path) = refused(&schema(vec![doc, team(false)]));
        assert_eq!(exclusion, "team#active");
        assert_eq!(path, "document#editor -> team#active");
    }

    #[test]
    fn a_tupleset_relations_own_rewrite_is_not_walked() {
        // The engine reads the tupleset raw, so an exclusion in its rewrite must not refuse.
        let doc = NamespaceIr::new(
            ns("document"),
            vec![
                RelationIr::direct(rel("member"), users()),
                RelationIr::direct(rel("banned"), users()),
                RelationIr::new(
                    rel("parent"),
                    RewriteIr::Union(vec![RewriteIr::This, but_not("member", "banned")]),
                    vec![RelatedType::leaf(ns("folder"))],
                ),
                RelationIr::new(
                    rel("viewer"),
                    RewriteIr::TupleToUserset {
                        tupleset: rel("parent"),
                        computed: rel("viewer"),
                    },
                    vec![],
                )
                .with_lock(),
            ],
        );
        let folder = NamespaceIr::new(
            ns("folder"),
            vec![RelationIr::direct(rel("viewer"), users())],
        );
        assert!(refuse_locked_exclusion(&schema(vec![doc, folder])).is_ok());
    }

    #[test]
    fn a_cycle_terminates_and_still_finds_the_exclusion() {
        let group = NamespaceIr::new(
            ns("group"),
            vec![
                RelationIr::direct(
                    rel("member"),
                    vec![
                        RelatedType::userset(ns("group"), rel("member")),
                        RelatedType::userset(ns("team"), rel("active")),
                    ],
                )
                .with_lock(),
            ],
        );
        let (_, _, exclusion, path) = refused(&schema(vec![group, team(false)]));
        assert_eq!(exclusion, "team#active");
        assert_eq!(path, "group#member -> team#active");
    }

    #[test]
    fn a_cycle_without_an_exclusion_terminates() {
        let group = NamespaceIr::new(
            ns("group"),
            vec![
                RelationIr::direct(
                    rel("member"),
                    vec![
                        RelatedType::leaf(ns("user")),
                        RelatedType::userset(ns("group"), rel("member")),
                    ],
                )
                .with_lock(),
            ],
        );
        assert!(refuse_locked_exclusion(&schema(vec![group])).is_ok());
    }
}
