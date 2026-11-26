//! [`LockedQuestions`]: the questions one locked check answers together.

use super::authorizer_ext::build_refs;
use crate::error::AuthzError;
use crate::schema::{Namespace, Relation, RelationName};
use crate::tuple::{IntoCanonicalId, IntoSubject, ObjectRef, SubjectRef};

/// One or more `object#relation@subject` questions, answered by one
/// [`check_in_locked`](super::TransactionalAuthorizer::check_in_locked).
///
///
/// Non-empty. Asking them in one call takes all their tuple locks as one ascending sequence,
/// so two acts cannot deadlock. It carries no caveat context.
#[derive(Clone, Debug, PartialEq)]
pub struct LockedQuestions(Vec<(ObjectRef, RelationName, SubjectRef)>);

impl LockedQuestions {
    /// The most tuples one locked check locks, across all of its questions.
    ///
    /// A larger witness is refused before any lock is taken.
    pub const WITNESS_LIMIT: usize = 64;

    /// Whether `subject` holds `relation` on `object`.
    #[must_use]
    pub fn new(object: ObjectRef, relation: RelationName, subject: SubjectRef) -> Self {
        Self(vec![(object, relation, subject)])
    }

    /// Whether `subject` holds the typed `relation` on `object` — the
    /// [`AuthorizerExt::can`](super::AuthorizerExt::can) form.
    ///
    /// # Errors
    ///
    /// [`AuthzError::InvalidName`] / [`AuthzError::InvalidCanonicalId`] if the subject or
    /// object does not name a valid reference.
    pub fn can<N: Namespace + Relation>(
        subject: impl IntoSubject,
        relation: N,
        object: impl IntoCanonicalId,
    ) -> Result<Self, AuthzError> {
        let (object, relation, subject) = build_refs::<N>(subject, relation, object)?;
        Ok(Self::new(object, relation, subject))
    }

    /// These questions and one more.
    #[must_use]
    pub fn and(mut self, object: ObjectRef, relation: RelationName, subject: SubjectRef) -> Self {
        self.0.push((object, relation, subject));
        self
    }

    /// These questions and one more, in the typed form.
    ///
    /// # Errors
    ///
    /// As [`can`](Self::can).
    pub fn and_can<N: Namespace + Relation>(
        self,
        subject: impl IntoSubject,
        relation: N,
        object: impl IntoCanonicalId,
    ) -> Result<Self, AuthzError> {
        let (object, relation, subject) = build_refs::<N>(subject, relation, object)?;
        Ok(self.and(object, relation, subject))
    }

    /// How many questions there are; never zero.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always `false`: a set of locked questions holds at least one.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The questions, in the order they were asked.
    pub fn iter(&self) -> impl Iterator<Item = &(ObjectRef, RelationName, SubjectRef)> {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::NamespaceName;
    use crate::tuple::CanonicalId;

    fn question(object: &str) -> (ObjectRef, RelationName, SubjectRef) {
        let ns = |n: &str| NamespaceName::new(n).expect("valid namespace");
        let id = |i: &str| CanonicalId::new(i).expect("valid id");
        (
            ObjectRef::new(ns("team"), id(object)),
            RelationName::new("member").expect("valid relation"),
            SubjectRef::leaf(ns("user"), id("alice")),
        )
    }

    #[test]
    fn questions_keep_the_order_they_were_asked_in() {
        let (a, b) = (question("a"), question("b"));
        let questions = LockedQuestions::new(b.0.clone(), b.1.clone(), b.2.clone()).and(
            a.0.clone(),
            a.1.clone(),
            a.2.clone(),
        );
        assert_eq!(questions.len(), 2);
        assert!(!questions.is_empty());
        assert_eq!(questions.iter().cloned().collect::<Vec<_>>(), vec![b, a]);
    }
}
