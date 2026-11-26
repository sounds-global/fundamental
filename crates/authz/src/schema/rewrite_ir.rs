//! [`RewriteIr`]: the userset-rewrite algebra the check engine evaluates.

use serde::{Deserialize, Serialize};

use super::RelationName;

/// A userset-rewrite expression: how a relation's effective subject set is
/// computed from stored tuples and other relations.
///
/// The Zanzibar rewrite algebra as data, so a config file can produce it too. Relation names
/// are within the relation's own namespace, except `TupleToUserset::computed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RewriteIr {
    /// `_this`: the subjects stored directly on this object#relation.
    This,
    /// The subjects of another relation **on the same object** (`computed_userset`).
    ComputedUserset {
        /// The sibling relation to consult.
        relation: RelationName,
    },
    /// For each object reached through `tupleset`, the subjects of its `computed` relation
    /// (`tuple_to_userset`).
    TupleToUserset {
        /// The relation on *this* object whose edges name the objects to hop to.
        tupleset: RelationName,
        /// The relation to evaluate on each hopped-to object.
        computed: RelationName,
    },
    /// Set union: a subject in any child set.
    Union(Vec<RewriteIr>),
    /// Set intersection: a subject in every child set.
    Intersection(Vec<RewriteIr>),
    /// Set difference: in `include` and not in `exclude`.
    Exclusion {
        /// The base set.
        include: Box<RewriteIr>,
        /// The set removed from the base.
        exclude: Box<RewriteIr>,
    },
}

impl RewriteIr {
    /// Collects the same-namespace relations this rewrite references directly.
    pub(crate) fn collect_local_relations<'a>(&'a self, out: &mut Vec<&'a RelationName>) {
        match self {
            RewriteIr::This => {}
            RewriteIr::ComputedUserset { relation } => out.push(relation),
            RewriteIr::TupleToUserset { tupleset, .. } => out.push(tupleset),
            RewriteIr::Union(children) | RewriteIr::Intersection(children) => {
                for child in children {
                    child.collect_local_relations(out);
                }
            }
            RewriteIr::Exclusion { include, exclude } => {
                include.collect_local_relations(out);
                exclude.collect_local_relations(out);
            }
        }
    }

    /// Collects every `(tupleset, computed)` pair in this rewrite.
    pub(crate) fn collect_tuple_to_usersets<'a>(
        &'a self,
        out: &mut Vec<(&'a RelationName, &'a RelationName)>,
    ) {
        match self {
            RewriteIr::This | RewriteIr::ComputedUserset { .. } => {}
            RewriteIr::TupleToUserset { tupleset, computed } => out.push((tupleset, computed)),
            RewriteIr::Union(children) | RewriteIr::Intersection(children) => {
                for child in children {
                    child.collect_tuple_to_usersets(out);
                }
            }
            RewriteIr::Exclusion { include, exclude } => {
                include.collect_tuple_to_usersets(out);
                exclude.collect_tuple_to_usersets(out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(name: &str) -> RelationName {
        RelationName::new(name).expect("valid relation")
    }

    fn sample() -> RewriteIr {
        RewriteIr::Union(vec![
            RewriteIr::ComputedUserset {
                relation: rel("editor"),
            },
            RewriteIr::TupleToUserset {
                tupleset: rel("parent"),
                computed: rel("viewer"),
            },
        ])
    }

    #[test]
    fn collects_local_relations() {
        let ir = sample();
        let mut out = Vec::new();
        ir.collect_local_relations(&mut out);
        assert!(out.contains(&&rel("editor")));
        assert!(out.contains(&&rel("parent")));
        assert!(
            !out.contains(&&rel("viewer")),
            "computed is cross-namespace, not local"
        );
    }

    #[test]
    fn collects_tuple_to_usersets() {
        let ir = sample();
        let mut out = Vec::new();
        ir.collect_tuple_to_usersets(&mut out);
        assert_eq!(out, vec![(&rel("parent"), &rel("viewer"))]);
    }

    #[test]
    fn round_trips_json() {
        let ir = sample();
        let json = serde_json::to_string(&ir).expect("serialize");
        let back: RewriteIr = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ir, back);
    }
}
