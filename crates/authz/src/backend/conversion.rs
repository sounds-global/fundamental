//! Shared conversion between a [`RelationTuple`] and its flat storage columns.

use crate::error::AuthzError;
use crate::schema::{CaveatName, NamespaceName, RelationName};
use crate::tuple::{CanonicalId, Caveat, CaveatContext, ObjectRef, RelationTuple, SubjectRef};

/// Encodes a tuple's caveat into `(caveat_name, caveat_context_json)`, or `None`.
///
/// # Errors
///
/// Returns [`AuthzError::Other`] if the caveat context fails to serialize.
pub(crate) fn caveat_to_columns(
    caveat: &Option<Caveat>,
) -> Result<(Option<String>, Option<String>), AuthzError> {
    match caveat {
        None => Ok((None, None)),
        Some(caveat) => {
            let context = if caveat.context.is_empty() {
                None
            } else {
                Some(
                    serde_json::to_string(&caveat.context)
                        .map_err(|e| AuthzError::CaveatEncoding(e.to_string()))?,
                )
            };
            Ok((Some(caveat.name.to_string()), context))
        }
    }
}

/// Reconstructs a validated [`RelationTuple`] from its stored columns.
///
/// Every field is re-validated, so corrupt storage fails closed. An empty `subject_relation`
/// is a principal leaf.
///
/// # Errors
///
/// Returns the specific [`AuthzError`] for the first field that fails validation,
/// or [`AuthzError::SchemaParse`] if the caveat context JSON is malformed.
#[allow(clippy::too_many_arguments)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
pub(crate) fn columns_to_tuple(
    object_ns: String,
    object_id: String,
    relation: String,
    subject_ns: String,
    subject_id: String,
    subject_relation: String,
    caveat_name: Option<String>,
    caveat_context: Option<String>,
) -> Result<RelationTuple, AuthzError> {
    let object = ObjectRef::new(NamespaceName::new(object_ns)?, CanonicalId::new(object_id)?);
    let relation = RelationName::new(relation)?;
    let subject_namespace = NamespaceName::new(subject_ns)?;
    let subject_object_id = CanonicalId::new(subject_id)?;
    let subject = if subject_relation.is_empty() {
        SubjectRef::leaf(subject_namespace, subject_object_id)
    } else {
        SubjectRef::userset(
            subject_namespace,
            subject_object_id,
            RelationName::new(subject_relation)?,
        )
    };

    let caveat = parse_caveat(caveat_name, caveat_context)?;

    Ok(RelationTuple {
        object,
        relation,
        subject,
        caveat,
    })
}

fn parse_caveat(
    name: Option<String>,
    context: Option<String>,
) -> Result<Option<Caveat>, AuthzError> {
    match name {
        None => Ok(None),
        Some(name) => {
            let name = CaveatName::new(name)?;
            let context = match context {
                None => CaveatContext::new(),
                Some(json) => serde_json::from_str(&json)
                    .map_err(|e| AuthzError::SchemaParse(e.to_string()))?,
            };
            Ok(Some(Caveat::new(name, context)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::CaveatValue;

    fn ns(name: &str) -> NamespaceName {
        NamespaceName::new(name).expect("valid")
    }
    fn id(value: &str) -> CanonicalId {
        CanonicalId::new(value).expect("valid")
    }

    #[test]
    fn round_trips_a_caveated_userset_tuple() {
        let original = RelationTuple::new(
            ObjectRef::new(ns("document"), id("readme")),
            RelationName::new("viewer").expect("valid"),
            SubjectRef::userset(
                ns("group"),
                id("eng"),
                RelationName::new("member").expect("valid"),
            ),
        )
        .with_caveat(Caveat::new(
            CaveatName::new("not_expired").expect("valid"),
            CaveatContext::new().with("valid_until", CaveatValue::Timestamp(100)),
        ));

        let (caveat_name, caveat_context) = caveat_to_columns(&original.caveat).expect("encode");
        let rebuilt = columns_to_tuple(
            original.object.namespace.to_string(),
            original.object.id.to_string(),
            original.relation.to_string(),
            original.subject.object.namespace.to_string(),
            original.subject.object.id.to_string(),
            original
                .subject
                .relation
                .as_ref()
                .map(|r| r.to_string())
                .unwrap_or_default(),
            caveat_name,
            caveat_context,
        )
        .expect("rebuild");

        assert_eq!(rebuilt, original);
    }

    #[test]
    fn round_trips_a_plain_leaf_tuple() {
        let original = RelationTuple::new(
            ObjectRef::new(ns("document"), id("readme")),
            RelationName::new("owner").expect("valid"),
            SubjectRef::leaf(ns("user"), id("alice")),
        );
        let (cn, cc) = caveat_to_columns(&original.caveat).expect("encode");
        assert_eq!((cn.clone(), cc.clone()), (None, None));
        let rebuilt = columns_to_tuple(
            "document".into(),
            "readme".into(),
            "owner".into(),
            "user".into(),
            "alice".into(),
            String::new(),
            cn,
            cc,
        )
        .expect("rebuild");
        assert_eq!(rebuilt, original);
    }

    #[test]
    fn rejects_corrupt_stored_namespace() {
        let err = columns_to_tuple(
            "bad ns".into(),
            "x".into(),
            "viewer".into(),
            "user".into(),
            "alice".into(),
            String::new(),
            None,
            None,
        )
        .expect_err("corrupt namespace must fail closed");
        assert!(matches!(
            err,
            AuthzError::InvalidName {
                kind: "namespace",
                ..
            }
        ));
    }
}
