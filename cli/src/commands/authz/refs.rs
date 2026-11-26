//! Parsing the `namespace:id` and `namespace:id#relation` arguments of the `fun authz` verbs.

use fundamental::authz::schema::{NamespaceName, RelationName};
use fundamental::authz::tuple::{CanonicalId, ObjectRef, SubjectRef};

use crate::CliError;

pub(super) fn parse_object(value: &str) -> Result<ObjectRef, CliError> {
    let (namespace, id) = split_ns_id(value, "object", "")?;
    Ok(ObjectRef::new(
        NamespaceName::new(namespace)?,
        CanonicalId::new(id)?,
    ))
}

/// A leaf `namespace:id`, or a userset `namespace:id#relation`.
pub(super) fn parse_subject(value: &str) -> Result<SubjectRef, CliError> {
    const HINT: &str = " (or `namespace:id#relation` for a userset)";
    let (namespace, rest) = split_ns_id(value, "subject", HINT)?;
    let namespace = NamespaceName::new(namespace)?;
    match rest.split_once('#') {
        Some((id, relation)) => Ok(SubjectRef::userset(
            namespace,
            CanonicalId::new(id)?,
            RelationName::new(relation)?,
        )),
        None => Ok(SubjectRef::leaf(namespace, CanonicalId::new(rest)?)),
    }
}

/// Splits on the first `:`, refusing an empty side; `kind` and `hint` shape the message.
fn split_ns_id<'a>(
    value: &'a str,
    kind: &'static str,
    hint: &'static str,
) -> Result<(&'a str, &'a str), CliError> {
    match value.split_once(':') {
        Some((namespace, id)) if !namespace.is_empty() && !id.is_empty() => Ok((namespace, id)),
        _ => Err(CliError::InvalidReference {
            kind,
            value: value.to_string(),
            userset_hint: hint,
        }),
    }
}

#[cfg(test)]
mod tests {
    use fundamental::Error;

    use super::*;

    #[test]
    fn parses_an_object() {
        let object = parse_object("document:readme").expect("valid object");
        assert_eq!(object.namespace.as_str(), "document");
        assert_eq!(object.id.as_str(), "readme");
    }

    #[test]
    fn parses_a_leaf_subject() {
        let subject = parse_subject("user:alice").expect("valid subject");
        assert!(!subject.is_userset());
        assert_eq!(subject.object.id.as_str(), "alice");
    }

    #[test]
    fn parses_a_userset_subject() {
        let subject = parse_subject("group:eng#member").expect("valid userset");
        assert!(subject.is_userset());
        assert_eq!(subject.relation.as_ref().unwrap().as_str(), "member");
    }

    #[test]
    fn rejects_a_reference_without_a_colon() {
        let err = parse_object("readme").expect_err("no namespace");
        assert!(matches!(
            err,
            CliError::InvalidReference { kind: "object", .. }
        ));
    }

    #[test]
    fn rejects_an_empty_side() {
        assert!(parse_object("document:").is_err());
        assert!(parse_object(":readme").is_err());
    }

    #[test]
    fn forwards_the_newtype_validation_error() {
        // A bad id is `AUTHZ_006`…
        let bad_id = parse_object("document:has a space").expect_err("invalid id");
        assert_eq!(bad_id.error_code(), "AUTHZ_006");
        let bad_namespace = parse_object("bad namespace:readme").expect_err("invalid namespace");
        assert_eq!(bad_namespace.error_code(), "AUTHZ_001");
    }
}
