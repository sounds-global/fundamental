//! [`LockedRelation`]: a relation named in settings, resolved against the schema at boot.

use fundamental_core::validate::{ValidationError, ValidationErrors};

use super::{NamespaceName, RelationName, SchemaIr};
#[cfg(feature = "sql")]
use crate::authorizer::LockedQuestions;
#[cfg(feature = "sql")]
use crate::tuple::{CanonicalId, ObjectRef, SubjectRef};

/// A relation the schema declares `#[locked]`, on the namespace it was resolved in.
///
/// Only [`resolve`](Self::resolve) builds one, so holding it proves the relation may be asked
/// in a locked check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedRelation {
    namespace: NamespaceName,
    relation: RelationName,
}

impl LockedRelation {
    /// The relation `raw` names on `namespace`, when `schema` declares it `#[locked]`.
    ///
    /// Each problem is added to `problems` under `field` (`relation_invalid`,
    /// `relation_undeclared` or `relation_not_locked`).
    pub fn resolve(
        schema: &SchemaIr,
        namespace: &NamespaceName,
        raw: &str,
        field: &str,
        problems: &mut ValidationErrors,
    ) -> Option<Self> {
        let Ok(relation) = RelationName::new(raw) else {
            problems.add(
                ValidationError::custom("relation_invalid", "not a relation name")
                    .with_field(field),
            );
            return None;
        };
        match schema.relation(namespace, &relation) {
            Some(declared) if declared.locked => Some(Self {
                namespace: namespace.clone(),
                relation,
            }),
            Some(_) => {
                problems.add(
                    ValidationError::custom(
                        "relation_not_locked",
                        format!("`{namespace}#{raw}` is not declared #[locked]"),
                    )
                    .with_field(field),
                );
                None
            }
            None => {
                problems.add(
                    ValidationError::custom(
                        "relation_undeclared",
                        format!("the schema declares no relation `{namespace}#{raw}`"),
                    )
                    .with_field(field),
                );
                None
            }
        }
    }

    /// The namespace the relation was resolved in.
    #[must_use]
    pub fn namespace(&self) -> &NamespaceName {
        &self.namespace
    }

    /// The relation's name.
    #[must_use]
    pub fn relation(&self) -> &RelationName {
        &self.relation
    }

    /// Whether `subject` holds this relation on `object`, as the first question of a locked
    /// check; further ones join with [`LockedQuestions::and`].
    #[cfg(feature = "sql")]
    #[must_use]
    pub fn question(&self, object: CanonicalId, subject: SubjectRef) -> LockedQuestions {
        LockedQuestions::new(
            ObjectRef::new(self.namespace.clone(), object),
            self.relation.clone(),
            subject,
        )
    }
}
