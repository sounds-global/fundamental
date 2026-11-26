//! [`CheckPath`]: the ancestor chain of one evaluation branch, for cycle detection.

use std::sync::Arc;

use crate::schema::RelationName;
use crate::tuple::{ObjectRef, SubjectRef};

/// The identity of one `check(object, relation, subject)` sub-question.
pub(super) type CheckKey = (ObjectRef, RelationName, SubjectRef);

/// The sub-questions on the stack above a branch, used to detect a cycle.
///
/// A persistent cons-list, so each concurrent sibling branch carries its own ancestors
/// cheaply; a shared visiting set would mistake a sibling's in-flight key for a cycle.
#[derive(Clone, Default)]
pub(super) struct CheckPath(Option<Arc<Frame>>);

struct Frame {
    key: CheckKey,
    parent: Option<Arc<Frame>>,
}

impl CheckPath {
    pub(super) fn root() -> Self {
        Self(None)
    }

    /// Whether `key` is already an ancestor of this branch, so extending into it closes a cycle.
    pub(super) fn contains(&self, key: &CheckKey) -> bool {
        let mut node = self.0.as_deref();
        while let Some(frame) = node {
            if &frame.key == key {
                return true;
            }
            node = frame.parent.as_deref();
        }
        false
    }

    #[must_use]
    pub(super) fn push(&self, key: CheckKey) -> Self {
        Self(Some(Arc::new(Frame {
            key,
            parent: self.0.clone(),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::NamespaceName;
    use crate::tuple::CanonicalId;

    fn key(id: &str) -> CheckKey {
        (
            ObjectRef::new(
                NamespaceName::new("doc").unwrap(),
                CanonicalId::new(id).unwrap(),
            ),
            RelationName::new("viewer").unwrap(),
            SubjectRef::leaf(
                NamespaceName::new("user").unwrap(),
                CanonicalId::new("alice").unwrap(),
            ),
        )
    }

    #[test]
    fn root_contains_nothing() {
        assert!(!CheckPath::root().contains(&key("a")));
    }

    #[test]
    fn push_records_an_ancestor_without_disturbing_the_parent() {
        let parent = CheckPath::root().push(key("a"));
        let child = parent.push(key("b"));
        assert!(child.contains(&key("a")));
        assert!(child.contains(&key("b")));
        assert!(parent.contains(&key("a")));
        assert!(!parent.contains(&key("b")));
    }

    #[test]
    fn siblings_do_not_see_each_other() {
        // Sibling branches must not observe each other's keys.
        let parent = CheckPath::root().push(key("root"));
        let left = parent.push(key("left"));
        let right = parent.push(key("right"));
        assert!(!left.contains(&key("right")));
        assert!(!right.contains(&key("left")));
    }
}
