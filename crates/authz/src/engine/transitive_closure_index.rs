//! [`TransitiveClosureIndex`]: an in-memory [`SetIndex`] over group membership closures.

use std::collections::{HashMap, HashSet, VecDeque};

use super::set_index::SetIndex;
use crate::schema::{NamespaceName, RelationName, RewriteIr, SchemaIr};
use crate::tuple::{ObjectRef, RelationTuple, SubjectRef};
use crate::zookie::HybridTs;

/// A batch-built transitive-membership index (Zanzibar's Leopard, in memory).
///
/// Covers self-referential membership relations (nested groups). A relation with any caveated
/// tuple, or a userset of another relation or namespace, is dropped from coverage. The
/// caller must supply the complete tuple set as of `watermark`.
#[derive(Debug, Clone)]
pub struct TransitiveClosureIndex {
    watermark: HybridTs,
    covered: HashSet<(NamespaceName, RelationName)>,
    closures: HashMap<(ObjectRef, RelationName), HashSet<SubjectRef>>,
}

impl TransitiveClosureIndex {
    /// Builds the index from the complete `tuples` for the indexed relations, as of
    /// `watermark`, validating each candidate relation against `schema`.
    ///
    /// Only `this` relations are candidates; see the type's coverage rule.
    #[must_use]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn build(schema: &SchemaIr, tuples: &[RelationTuple], watermark: HybridTs) -> Self {
        let mut disqualified: HashSet<(NamespaceName, RelationName)> = HashSet::new();
        let mut candidates: HashSet<(NamespaceName, RelationName)> = HashSet::new();
        let mut edges: HashMap<(ObjectRef, RelationName), Vec<SubjectRef>> = HashMap::new();

        for tuple in tuples {
            let key = (tuple.object.namespace.clone(), tuple.relation.clone());
            if disqualified.contains(&key) {
                continue;
            }
            if !Self::is_candidate(schema, &tuple.object.namespace, &tuple.relation) {
                continue;
            }
            candidates.insert(key.clone());

            // A conditional edge cannot be precomputed, so the relation is dropped.
            if tuple.caveat.is_some() {
                Self::disqualify(&key, &mut disqualified, &mut candidates, &mut edges);
                continue;
            }
            if tuple.subject.is_userset() {
                // The closure follows only a userset of the same namespace and relation.
                let same_shape = tuple.subject.relation.as_ref() == Some(&tuple.relation)
                    && tuple.subject.object.namespace == tuple.object.namespace;
                if !same_shape {
                    Self::disqualify(&key, &mut disqualified, &mut candidates, &mut edges);
                    continue;
                }
            }
            edges
                .entry((tuple.object.clone(), tuple.relation.clone()))
                .or_default()
                .push(tuple.subject.clone());
        }

        let covered: HashSet<(NamespaceName, RelationName)> =
            candidates.difference(&disqualified).cloned().collect();

        let mut closures: HashMap<(ObjectRef, RelationName), HashSet<SubjectRef>> = HashMap::new();
        for (object, relation) in edges.keys() {
            let key = (object.namespace.clone(), relation.clone());
            if !covered.contains(&key) {
                continue;
            }
            let leaves = Self::closure(object, relation, &edges);
            closures.insert((object.clone(), relation.clone()), leaves);
        }

        Self {
            watermark,
            covered,
            closures,
        }
    }

    /// Whether `namespace#relation` is a `this`-only relation.
    fn is_candidate(schema: &SchemaIr, namespace: &NamespaceName, relation: &RelationName) -> bool {
        schema
            .relation(namespace, relation)
            .is_some_and(|ir| ir.rewrite == RewriteIr::This)
    }

    fn disqualify(
        key: &(NamespaceName, RelationName),
        disqualified: &mut HashSet<(NamespaceName, RelationName)>,
        candidates: &mut HashSet<(NamespaceName, RelationName)>,
        edges: &mut HashMap<(ObjectRef, RelationName), Vec<SubjectRef>>,
    ) {
        disqualified.insert(key.clone());
        candidates.remove(key);
        edges.retain(|(object, relation), _| !(object.namespace == key.0 && *relation == key.1));
    }

    /// The leaf subjects reachable from `object#relation`, terminating on a cycle.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn closure(
        object: &ObjectRef,
        relation: &RelationName,
        edges: &HashMap<(ObjectRef, RelationName), Vec<SubjectRef>>,
    ) -> HashSet<SubjectRef> {
        let mut leaves = HashSet::new();
        let mut visited: HashSet<ObjectRef> = HashSet::new();
        let mut frontier: VecDeque<ObjectRef> = VecDeque::new();
        frontier.push_back(object.clone());
        visited.insert(object.clone());

        while let Some(current) = frontier.pop_front() {
            let Some(subjects) = edges.get(&(current, relation.clone())) else {
                continue;
            };
            for subject in subjects {
                if subject.is_userset() {
                    if visited.insert(subject.object.clone()) {
                        frontier.push_back(subject.object.clone());
                    }
                } else {
                    leaves.insert(subject.clone());
                }
            }
        }
        leaves
    }
}

impl SetIndex for TransitiveClosureIndex {
    fn watermark(&self) -> HybridTs {
        self.watermark
    }

    fn covers(&self, namespace: &NamespaceName, relation: &RelationName) -> bool {
        self.covered
            .contains(&(namespace.clone(), relation.clone()))
    }

    fn contains(
        &self,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
    ) -> Option<bool> {
        if !self.covers(&object.namespace, relation) {
            return None;
        }
        // A userset or wildcard subject is left to the engine.
        if subject.is_userset() || subject.is_wildcard() {
            return None;
        }
        let member = self
            .closures
            .get(&(object.clone(), relation.clone()))
            .is_some_and(|leaves| leaves.contains(subject));
        Some(member)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::schema::{NamespaceIr, RelatedType, RelationIr};
    use crate::tuple::CanonicalId;

    fn nn(s: &str) -> NamespaceName {
        NamespaceName::new(s).unwrap()
    }
    fn rn(s: &str) -> RelationName {
        RelationName::new(s).unwrap()
    }
    fn group(id: &str) -> ObjectRef {
        ObjectRef::new(nn("group"), CanonicalId::new(id).unwrap())
    }
    fn user(id: &str) -> SubjectRef {
        SubjectRef::leaf(nn("user"), CanonicalId::new(id).unwrap())
    }
    fn group_member(id: &str) -> SubjectRef {
        SubjectRef::userset(nn("group"), CanonicalId::new(id).unwrap(), rn("member"))
    }
    fn member_tuple(object: ObjectRef, subject: SubjectRef) -> RelationTuple {
        RelationTuple::new(object, rn("member"), subject)
    }

    fn schema() -> SchemaIr {
        let user = NamespaceIr::new(nn("user"), vec![]);
        let group = NamespaceIr::new(
            nn("group"),
            vec![RelationIr::direct(
                rn("member"),
                vec![
                    RelatedType::leaf(nn("user")),
                    RelatedType::userset(nn("group"), rn("member")),
                ],
            )],
        );
        SchemaIr::compile(BTreeMap::from([(nn("user"), user), (nn("group"), group)]))
            .expect("valid schema")
    }

    fn watermark() -> HybridTs {
        HybridTs::new(100, 0)
    }

    fn chain(depth: usize) -> Vec<RelationTuple> {
        let mut tuples = vec![member_tuple(group("g0"), user("alice"))];
        for i in 0..depth {
            tuples.push(member_tuple(
                group(&format!("g{}", i + 1)),
                group_member(&format!("g{i}")),
            ));
        }
        tuples
    }

    #[test]
    fn answers_deep_membership_in_one_lookup() {
        let index = TransitiveClosureIndex::build(&schema(), &chain(32), watermark());
        assert!(index.covers(&nn("group"), &rn("member")));
        assert_eq!(
            index.contains(&group("g32"), &rn("member"), &user("alice")),
            Some(true)
        );
        assert_eq!(
            index.contains(&group("g0"), &rn("member"), &user("alice")),
            Some(true)
        );
    }

    #[test]
    fn reports_authoritative_non_membership() {
        let index = TransitiveClosureIndex::build(&schema(), &chain(4), watermark());
        assert_eq!(
            index.contains(&group("g4"), &rn("member"), &user("mallory")),
            Some(false)
        );
        assert_eq!(
            index.contains(&group("empty"), &rn("member"), &user("alice")),
            Some(false)
        );
    }

    #[test]
    fn terminates_on_a_membership_cycle() {
        // A cycle: the closure must not loop.
        let tuples = vec![
            member_tuple(group("a"), group_member("b")),
            member_tuple(group("b"), group_member("a")),
            member_tuple(group("a"), user("alice")),
        ];
        let index = TransitiveClosureIndex::build(&schema(), &tuples, watermark());
        assert_eq!(
            index.contains(&group("b"), &rn("member"), &user("alice")),
            Some(true)
        );
    }

    #[test]
    fn a_caveated_edge_drops_the_relation_from_coverage() {
        use crate::tuple::{Caveat, CaveatContext};
        let mut caveated = member_tuple(group("g0"), user("alice"));
        caveated.caveat = Some(Caveat::new(
            crate::schema::CaveatName::new("business_hours").unwrap(),
            CaveatContext::new(),
        ));
        let index = TransitiveClosureIndex::build(&schema(), &[caveated], watermark());
        assert!(!index.covers(&nn("group"), &rn("member")));
        assert_eq!(
            index.contains(&group("g0"), &rn("member"), &user("alice")),
            None
        );
    }

    #[test]
    fn a_userset_subject_is_left_to_the_engine() {
        let index = TransitiveClosureIndex::build(&schema(), &chain(2), watermark());
        assert_eq!(
            index.contains(&group("g2"), &rn("member"), &group_member("g1")),
            None
        );
    }
}
