use crate::SqlError;
use fundamental_db::{ConstraintKind, DatabaseError};

/// The constraint a write violated: its kind, and its name where the engine gave one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViolatedConstraint {
    /// Unique, foreign key, not-null or check.
    pub kind: ConstraintKind,
    /// The constraint's name, or `None` when neither the driver nor the engine's message carried one.
    pub name: Option<String>,
}

impl ViolatedConstraint {
    /// Reads the violation out of `error`, including a `SqlError` erased into [`Other`](DatabaseError::Other).
    #[must_use]
    pub fn of(error: &DatabaseError) -> Option<Self> {
        match error {
            DatabaseError::ConstraintViolation { constraint, kind } => Some(Self {
                kind: *kind,
                name: (!constraint.is_empty()).then(|| constraint.clone()),
            }),
            DatabaseError::Other(any) => any.downcast_ref::<SqlError>().and_then(Self::of_sql),
            _ => None,
        }
    }

    /// Reads the violation out of an adapter-level `error`.
    #[must_use]
    pub fn of_sql(error: &SqlError) -> Option<Self> {
        match error {
            SqlError::ConstraintViolation {
                constraint, kind, ..
            } => Some(Self {
                kind: *kind,
                name: constraint.clone(),
            }),
            _ => None,
        }
    }

    /// Whether this is a unique violation of the constraint named `name`.
    #[must_use]
    pub fn is_unique(&self, name: &str) -> bool {
        self.kind == ConstraintKind::Unique && self.name.as_deref() == Some(name)
    }

    /// Whether this is a foreign-key violation of the constraint named `name`.
    #[must_use]
    pub fn is_foreign_key(&self, name: &str) -> bool {
        self.kind == ConstraintKind::ForeignKey && self.name.as_deref() == Some(name)
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::AnyError;
    use fundamental_db::{ConstraintKind, DatabaseError};

    use super::ViolatedConstraint;
    use crate::SqlError;

    fn unique(name: Option<&str>) -> SqlError {
        SqlError::ConstraintViolation {
            message: "duplicate key".to_owned(),
            table: None,
            constraint: name.map(str::to_owned),
            kind: ConstraintKind::Unique,
        }
    }

    #[test]
    fn a_violation_is_read_through_the_erased_adapter_error() {
        let erased =
            DatabaseError::Other(AnyError::new(unique(Some("tenant_entities_by_skeleton"))));
        assert_eq!(
            erased.constraint_kind(),
            None,
            "the enum's own accessor cannot see it"
        );
        let violated = ViolatedConstraint::of(&erased).expect("a violation");
        assert!(violated.is_unique("tenant_entities_by_skeleton"));
        assert!(!violated.is_unique("tenant_entities_by_slug"));
    }

    #[test]
    fn a_foreign_key_violation_is_named_apart_from_a_unique_one() {
        let erased = DatabaseError::Other(AnyError::new(SqlError::ConstraintViolation {
            message: "violates foreign key constraint".to_owned(),
            table: None,
            constraint: Some("aliases_look_belongs_to_the_namespace".to_owned()),
            kind: ConstraintKind::ForeignKey,
        }));
        let violated = ViolatedConstraint::of(&erased).expect("a violation");
        assert!(violated.is_foreign_key("aliases_look_belongs_to_the_namespace"));
        assert!(!violated.is_unique("aliases_look_belongs_to_the_namespace"));
        assert!(!violated.is_foreign_key("another_key"));
        assert!(
            !ViolatedConstraint::of(&DatabaseError::Other(AnyError::new(unique(Some("k")))))
                .expect("a violation")
                .is_foreign_key("k")
        );
    }

    #[test]
    fn the_abstract_variant_is_read_too() {
        let direct = DatabaseError::unique_violation("users_email_key");
        assert!(
            ViolatedConstraint::of(&direct)
                .expect("a violation")
                .is_unique("users_email_key")
        );
    }

    #[test]
    fn anything_else_is_no_violation() {
        assert_eq!(ViolatedConstraint::of(&DatabaseError::timeout(10)), None);
        assert_eq!(
            ViolatedConstraint::of(&DatabaseError::Other(AnyError::new(SqlError::query("x")))),
            None
        );
    }
}
