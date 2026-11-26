//! [`LockedDecision`]: what a locked check answered.

/// The answers of one [`check_in_locked`](super::TransactionalAuthorizer::check_in_locked),
/// in the order its questions were asked.
///
///
/// Every allowed answer rests on tuples locked until the caller's transaction ends.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use = "a locked check that is not acted on held its locks for nothing"]
pub struct LockedDecision(Vec<bool>);

impl LockedDecision {
    pub(crate) fn new(answers: Vec<bool>) -> Self {
        Self(answers)
    }

    /// Whether every question was allowed.
    #[must_use]
    pub fn all(&self) -> bool {
        self.0.iter().all(|allowed| *allowed)
    }

    /// Whether the question at `index` was allowed, or `None` past the last question.
    #[must_use]
    pub fn allowed(&self, index: usize) -> Option<bool> {
        self.0.get(index).copied()
    }

    /// Every answer, in question order.
    pub fn iter(&self) -> impl Iterator<Item = bool> + '_ {
        self.0.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_is_every_answer_allowed() {
        assert!(LockedDecision::new(vec![true, true]).all());
        let mixed = LockedDecision::new(vec![true, false]);
        assert!(!mixed.all());
        assert_eq!(mixed.allowed(1), Some(false));
        assert_eq!(mixed.allowed(2), None);
        assert_eq!(mixed.iter().collect::<Vec<_>>(), vec![true, false]);
    }
}
