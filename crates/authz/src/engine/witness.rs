//! [`Witness`]: the stored tuples an allow rests on.

use std::collections::BTreeMap;
use std::collections::btree_map::Values;

use super::Justification;
use crate::tuple::{RelationTuple, TupleKey};

/// The tuples one granting derivation used, keyed and so ordered by [`TupleKey`].
///
/// Without exclusion every operator is monotone, so these tuples suffice; locking them in key
/// order keeps two locked checks from deadlocking.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Witness(BTreeMap<TupleKey, RelationTuple>);

impl Witness {
    /// How many tuples the witness holds.
    #[cfg_attr(
        not(feature = "sql"),
        expect(dead_code, reason = "only the `sql` engine reads it")
    )]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// The tuples, in ascending key order — the order they are locked in.
    #[cfg_attr(
        not(feature = "sql"),
        expect(dead_code, reason = "only the `sql` engine reads it")
    )]
    pub(crate) fn tuples(&self) -> Values<'_, TupleKey, RelationTuple> {
        self.0.values()
    }

    /// Adds every tuple of `other`, for the questions one call answers together.
    pub(crate) fn absorb(&mut self, other: Self) {
        self.0.extend(other.0);
    }
}

impl Justification for Witness {
    const ADMITS_EXCLUSION: bool = false;

    fn unit() -> Self {
        Self::default()
    }

    fn edge(tuple: impl FnOnce() -> RelationTuple) -> Self {
        let tuple = tuple();
        Self(BTreeMap::from([(tuple.key(), tuple)]))
    }

    fn join(mut self, other: Self) -> Self {
        self.absorb(other);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{NamespaceName, RelationName};
    use crate::tuple::{CanonicalId, ObjectRef, SubjectRef};

    fn tuple(object: &str, subject: &str) -> RelationTuple {
        let ns = |n: &str| NamespaceName::new(n).expect("valid namespace");
        let id = |i: &str| CanonicalId::new(i).expect("valid id");
        RelationTuple::new(
            ObjectRef::new(ns("team"), id(object)),
            RelationName::new("member").expect("valid relation"),
            SubjectRef::leaf(ns("user"), id(subject)),
        )
    }

    #[test]
    fn witness_locks_ascend_whatever_the_walk_order() {
        let (a, b, c) = (tuple("a", "x"), tuple("b", "x"), tuple("c", "x"));
        let one = Witness::edge(|| c.clone())
            .join(Witness::edge(|| a.clone()))
            .join(Witness::edge(|| b.clone()));
        let mirrored = Witness::edge(|| b.clone())
            .join(Witness::edge(|| a.clone()).join(Witness::edge(|| c.clone())));

        let keys = |w: &Witness| w.tuples().map(RelationTuple::key).collect::<Vec<_>>();
        assert_eq!(keys(&one), keys(&mirrored));
        let mut sorted = keys(&one);
        sorted.sort();
        assert_eq!(keys(&one), sorted);
        assert_eq!(one.len(), 3);
    }

    #[test]
    fn one_tuple_reached_twice_is_locked_once() {
        let a = tuple("a", "x");
        let witness = Witness::edge(|| a.clone()).join(Witness::edge(|| a.clone()));
        assert_eq!(witness.len(), 1);
    }
}
