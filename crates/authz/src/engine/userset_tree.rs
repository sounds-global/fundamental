//! [`UsersetTree`]: the expansion of who holds a relation, as a tree.

use serde::{Deserialize, Serialize};

use crate::tuple::SubjectRef;

/// The result of an `expand`: the effective subject set of an `object#relation`,
/// as a tree that mirrors the relation's rewrite.
///
/// A stored userset appears as an unexpanded leaf, so the tree is finite.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum UsersetTree {
    /// Directly stored subjects (principal leaves and unexpanded usersets).
    Leaf(Vec<SubjectRef>),
    /// Union of child sets.
    Union(Vec<UsersetTree>),
    /// Intersection of child sets.
    Intersection(Vec<UsersetTree>),
    /// Set difference: in `include` and not in `exclude`.
    Exclusion {
        /// The base set.
        include: Box<UsersetTree>,
        /// The set removed from the base.
        exclude: Box<UsersetTree>,
    },
}

impl UsersetTree {
    /// Collects every subject in the tree, deduplicated in first-seen order.
    ///
    /// Ignores `intersection`/`exclusion`, so it is not an access answer; use
    /// [`check`](crate::engine::Engine::check).
    #[must_use]
    pub fn all_subjects(&self) -> Vec<SubjectRef> {
        let mut out = Vec::new();
        self.collect_subjects(&mut out);
        out
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn collect_subjects(&self, out: &mut Vec<SubjectRef>) {
        match self {
            UsersetTree::Leaf(subjects) => {
                for subject in subjects {
                    if !out.contains(subject) {
                        out.push(subject.clone());
                    }
                }
            }
            UsersetTree::Union(children) | UsersetTree::Intersection(children) => {
                for child in children {
                    child.collect_subjects(out);
                }
            }
            UsersetTree::Exclusion { include, exclude } => {
                include.collect_subjects(out);
                exclude.collect_subjects(out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::NamespaceName;
    use crate::tuple::CanonicalId;

    fn user(id: &str) -> SubjectRef {
        SubjectRef::leaf(
            NamespaceName::new("user").expect("ns"),
            CanonicalId::new(id).expect("id"),
        )
    }

    #[test]
    fn all_subjects_flattens_and_dedupes() {
        let tree = UsersetTree::Union(vec![
            UsersetTree::Leaf(vec![user("a"), user("b")]),
            UsersetTree::Leaf(vec![user("b"), user("c")]),
        ]);
        let subjects = tree.all_subjects();
        assert_eq!(subjects, vec![user("a"), user("b"), user("c")]);
    }
}
